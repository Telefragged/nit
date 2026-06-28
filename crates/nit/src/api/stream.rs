//! Events (WS `/api/stream`): the client-driven per-change change stream.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_broadcast::{Receiver, RecvError};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use tokio_stream::{StreamExt, StreamMap};

use nit_types::events::{ClientMsg, StreamMsg};
use nit_types::log::LogEntry;

use crate::db;
use crate::review;

use super::{AppState, with_conn};

/// `WS /api/stream?repo={id}` — the client-driven change stream
/// (docs/api.md "Events"). The `repo` query is accepted for symmetry and
/// ignored; the server keys purely on the subscribed change ids.
pub(super) async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// A per-change feed that **surfaces** broadcast overflow instead of silently
/// skipping past it. `async_broadcast`'s own `Stream` impl swallows
/// `Overflowed` (it `continue`s to the next slot), which would let a follower
/// that fell more than `EVENTS_BUFFER` behind lose entries with no signal.
/// Yielding the error lets `handle_socket` close the socket so the client
/// reconnects and re-reads the gap from the log (docs/api.md "Events").
struct Feed(Receiver<LogEntry>);

impl tokio_stream::Stream for Feed {
    type Item = Result<LogEntry, RecvError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_recv(cx)
    }
}

/// Drive one follower's socket: `subscribe` messages drive a keyed
/// `StreamMap` of per-change feeds (dynamic membership); each arms the feed
/// **before** reading the change's backlog (a `[from, head)` replay, or a
/// `ChangeProj` snapshot) and records an idx watermark so the arm/read overlap
/// is deduped, never gapped. A feed that overflows closes the socket — the
/// client reconnects and re-reads the log.
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut feeds: StreamMap<u64, Feed> = StreamMap::new();
    let mut watermark: HashMap<u64, u64> = HashMap::new();
    let mut shutdown = state.shutdown_watch();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(msg)) = incoming else { break };
                match msg {
                    Message::Text(text) => {
                        let Ok(client) = serde_json::from_str::<ClientMsg>(&text) else {
                            continue;
                        };
                        if apply_client_msg(&mut socket, &state, &mut feeds, &mut watermark, client)
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {} // ping/pong/binary: ignored
                }
            }
            Some((change_id, item)) = feeds.next(), if !feeds.is_empty() => {
                // Overflow (or a closed feed): this follower fell behind. Close
                // the socket so it reconnects and re-reads the gap from the log.
                let Ok(entry) = item else { break };
                if entry.idx < watermark.get(&change_id).copied().unwrap_or(0) {
                    continue;
                }
                if send(&mut socket, &StreamMsg::Entry(entry)).await.is_err() {
                    break;
                }
            }
            // The only change to the shutdown signal is false → true.
            _ = shutdown.changed() => break,
        }
    }
}

/// Apply one client message; `Err(())` means the socket should close.
async fn apply_client_msg(
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    feeds: &mut StreamMap<u64, Feed>,
    watermark: &mut HashMap<u64, u64>,
    client: ClientMsg,
) -> Result<(), ()> {
    match client {
        ClientMsg::Subscribe(map) => {
            for (id_str, from) in map {
                let Ok(change_id) = id_str.parse::<u64>() else {
                    continue;
                };
                let Some(entry) = state.change_entry(change_id) else {
                    continue;
                };
                // Subscribe before reading the backlog, so events that land
                // mid-read are caught here and deduped by the watermark.
                feeds.insert(change_id, Feed(entry.subscribe()));
                let backlog = read_backlog(state, change_id, from).await;
                let mut next = from;
                for e in backlog {
                    next = e.idx + 1;
                    send(socket, &StreamMsg::Entry(e)).await?;
                }
                watermark.insert(change_id, next);
            }
        }
        ClientMsg::SubscribeSnapshot(ids) => {
            for change_id in ids {
                let Some(entry) = state.change_entry(change_id) else {
                    continue;
                };
                // Arm the feed, then snapshot the projection: an append that
                // lands between the two rides the feed and is deduped by the
                // high-water mark, so the snapshot and its live tail neither
                // gap nor double (docs/api.md "Events"). Clone out from under
                // the read lock — no guard is held across the send.
                feeds.insert(change_id, Feed(entry.subscribe()));
                let proj = entry.read().clone();
                watermark.insert(change_id, proj.entries_folded);
                send(socket, &StreamMsg::Snapshot(proj)).await?;
            }
        }
    }
    Ok(())
}

async fn send(socket: &mut WebSocket, msg: &StreamMsg) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// A change's log slice `[from, head)` as tagged entries, for the backlog
/// replay. Errors collapse to empty (the follower re-reads on reconnect).
async fn read_backlog(state: &Arc<AppState>, change_id: u64, from: u64) -> Vec<LogEntry> {
    with_conn(state.pool(), move |conn| {
        let rows = db::log_entries(conn, change_id, from, None)?;
        rows.iter()
            .map(|r| review::entry_from_row(change_id, r))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(Into::into)
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use nit_types::enums::LifecycleAction;
    use nit_types::log::LogPayload;

    use super::*;

    /// `Feed` yields `Err(Overflowed)` when the broadcast buffer is full, not a
    /// silent skip.
    #[tokio::test]
    async fn feed_surfaces_overflow() {
        let (mut tx, rx) = async_broadcast::broadcast::<LogEntry>(2);
        tx.set_overflow(true);
        let mut feed = Feed(rx);
        // Overflow mode drops the oldest slot; the reader sees
        // RecvError::Overflowed, not the sender.
        for idx in 0..3 {
            let _ = tx.try_broadcast(LogEntry {
                change_id: 0,
                idx,
                seq: idx,
                created_at: String::new(),
                payload: LogPayload::lifecycle(LifecycleAction::Reopened, None, None),
            });
        }
        assert!(matches!(
            feed.next().await,
            Some(Err(RecvError::Overflowed(_)))
        ));
    }
}
