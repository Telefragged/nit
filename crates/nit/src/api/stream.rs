//! Events (WS `/api/stream`): the client-driven per-change change stream.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

use nit_types::events::{ClientMsg, StreamMsg};
use nit_types::log::LogEntry;

use crate::db;
use crate::review::{self, ChangeProj};

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
            let cursors = map
                .iter()
                .filter_map(|(id, from)| Some((id.parse::<u64>().ok()?, *from)))
                .collect();
            for (change_id, next, backlog) in read_backlogs(state, cursors).await {
                watermark.insert(change_id, next);
                for e in backlog {
                    send(socket, &StreamMsg::Entry(e)).await?;
                }
            }
        }
        ClientMsg::SubscribeSnapshot(ids) => {
            for (change_id, proj) in read_snapshots(state, ids).await {
                // The snapshot's `entries_folded` is the high-water mark, so an
                // append that lands after it rides the channel and is deduped
                // there: the snapshot and its live tail neither gap nor double
                // (docs/api.md "Events").
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

/// Each cursor's log slice `[from, head)` as tagged entries and the idx that
/// slice ends at, over one borrowed
/// connection — a subscribe carries a whole chain, so the frames it answers
/// with are sent after the read rather than between two of them. A change left
/// out of the result is left unsubscribed: it does not exist, or the read
/// failed and the follower re-reads on reconnect.
async fn read_backlogs(
    state: &Arc<AppState>,
    cursors: Vec<(u64, u64)>,
) -> Vec<(u64, u64, Vec<LogEntry>)> {
    with_conn(state.pool(), move |conn| {
        let mut out = Vec::with_capacity(cursors.len());
        for (change_id, from) in cursors {
            // Existence is a row read: cursor mode replays the log itself and
            // never touches the fold.
            if db::get_change(conn, change_id)?.is_none() {
                continue;
            }
            let rows = db::log_entries(conn, change_id, from, None)?;
            let entries = rows
                .iter()
                .map(|r| review::entry_from_row(change_id, r))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let next = entries.last().map_or(from, |e| e.idx + 1);
            out.push((change_id, next, entries));
        }
        Ok(out)
    })
    .await
    .unwrap_or_default()
}

/// Each change's folded projection, cloned out from under its read lock (no
/// guard is held across a send) — the one place a fold is resolved without a
/// connection already in hand, so it borrows one for the whole batch.
async fn read_snapshots(state: &Arc<AppState>, ids: Vec<u64>) -> Vec<(u64, ChangeProj)> {
    let st = state.clone();
    with_conn(state.pool(), move |conn| {
        let mut out = Vec::with_capacity(ids.len());
        for change_id in ids {
            if let Some(entry) = st.change(conn, change_id)? {
                out.push((change_id, entry.read().clone()));
            }
        }
        Ok(out)
    })
    .await
    .unwrap_or_default()
}
