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

use crate::db;
use crate::review;

use super::types;
use super::types::StreamMsg;
use super::views;
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
struct Feed(Receiver<StreamMsg>);

impl tokio_stream::Stream for Feed {
    type Item = Result<StreamMsg, RecvError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_recv(cx)
    }
}

/// Drive one follower's socket: `subscribe` messages drive a keyed
/// `StreamMap` of per-change feeds (dynamic membership); each arms the feed
/// **before** replaying the change's `[from, head)` backlog and records an idx
/// watermark so the arm/read overlap is deduped, never gapped. A feed that
/// overflows closes the socket — the client reconnects and re-reads the log.
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
                        let Ok(client) = serde_json::from_str::<types::ClientMsg>(&text) else {
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
                let Ok(msg) = item else { break };
                // Drop a live entry the backlog replay already covered.
                if let StreamMsg::Entry(ref e) = msg
                    && e.idx < watermark.get(&change_id).copied().unwrap_or(0)
                {
                    continue;
                }
                if send_json(&mut socket, &msg).await.is_err() {
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
    client: types::ClientMsg,
) -> Result<(), ()> {
    match client {
        types::ClientMsg::Subscribe(map) => {
            for (id_str, from) in map {
                let Ok(change_id) = id_str.parse::<u64>() else {
                    continue;
                };
                let Some(entry) = state.change_entry(change_id) else {
                    continue;
                };
                // Arm the live feed BEFORE reading the backlog.
                feeds.insert(change_id, Feed(entry.subscribe()));
                let backlog = read_backlog(state, change_id, from).await;
                let mut next = from;
                for e in &backlog {
                    next = e.idx + 1;
                    send_json(socket, &StreamMsg::Entry(e.clone())).await?;
                }
                watermark.insert(change_id, next);
            }
        }
    }
    Ok(())
}

async fn send_json(socket: &mut WebSocket, msg: &StreamMsg) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// A change's log slice `[from, head)` as tagged entries, for the backlog
/// replay. Errors collapse to empty (the follower re-reads on reconnect).
async fn read_backlog(state: &Arc<AppState>, change_id: u64, from: u64) -> Vec<types::LogEntry> {
    with_conn(state.pool(), move |conn| {
        let rows = db::log_entries(conn, change_id, from, None)?;
        rows.iter()
            .map(|r| {
                Ok(views::log_entry_view(
                    change_id,
                    &review::Entry::from_row(r)?,
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(Into::into)
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Feed` surfaces broadcast overflow as `Err` rather than silently
    /// skipping the gap — the signal `handle_socket` turns into a socket close.
    #[tokio::test]
    async fn feed_surfaces_overflow() {
        let (mut tx, rx) = async_broadcast::broadcast::<StreamMsg>(2);
        tx.set_overflow(true);
        let mut feed = Feed(rx);
        // Capacity 2, three sends: the oldest is dropped, overflowing the reader.
        for of in 0..3 {
            let _ = tx.try_broadcast(StreamMsg::NewParent {
                new_parent: types::NewParent { of, parent: 0 },
            });
        }
        assert!(matches!(
            feed.next().await,
            Some(Err(RecvError::Overflowed(_)))
        ));
    }
}
