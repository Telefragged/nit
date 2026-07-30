//! Events (WS `/api/stream`): the client-driven per-change change stream.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

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

/// Drive one follower's socket. It holds one receiver on the server's event
/// channel for its whole life, so every subscribe is armed before it reads its
/// backlog (a `[from, head)` replay, or a `ChangeProj` snapshot) and the
/// arm/read overlap is deduped by an idx watermark, never gapped. `watermark`
/// is also the subscription set: an entry is forwarded only for a change the
/// client asked for. An overflowed receiver closes the socket — the client
/// reconnects and re-reads the log.
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut events = state.subscribe();
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
                        if apply_client_msg(&mut socket, &state, &mut watermark, client)
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            item = events.recv() => {
                // Overflow (or a closed channel): this follower fell behind.
                // Close the socket so it reconnects and re-reads the gap from
                // the log.
                let Ok(entry) = item else { break };
                let Some(&mark) = watermark.get(&entry.change_id) else {
                    continue;
                };
                if entry.idx < mark {
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
    watermark: &mut HashMap<u64, u64>,
    client: ClientMsg,
) -> Result<(), ()> {
    match client {
        ClientMsg::Subscribe(map) => {
            for (id_str, from) in map {
                let Ok(change_id) = id_str.parse::<u64>() else {
                    continue;
                };
                if state.change_entry(change_id).is_none() {
                    continue;
                }
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
                // The snapshot's `entries_folded` is the high-water mark, so an
                // append that lands after it rides the channel and is deduped
                // there: the snapshot and its live tail neither gap nor double
                // (docs/api.md "Events"). Clone out from under the read lock —
                // no guard is held across the send.
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
