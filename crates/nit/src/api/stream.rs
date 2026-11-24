//! Events (WS `/api/stream`): the log of the changes that a query matches.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use tokio::sync::watch;

use nit_types::domain::ChangeNumber;
use nit_types::events::{Start, StreamMessage, Subscription};

use crate::review;

use super::state::changes_matching;
use super::{AppState, Error, with_conn};

/// `WS /api/stream`: the log of the changes that a query matches.
///
/// `nit_types::events::Subscription` documents the contract.
#[utoipa::path(get, path = "/api/stream", responses((status = 101, description = "A websocket of `StreamMessage` frames")))]
pub(super) async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let appended = state.appended_watch();
        follow(socket, state, appended, read)
    })
}

/// Runs the loop of a client socket.
///
/// If a read fails, the loop closes the socket. The client then subscribes
/// again.
pub(super) async fn follow<S, F, Fut>(
    mut socket: WebSocket,
    state: Arc<AppState>,
    mut changed: watch::Receiver<()>,
    read: F,
) where
    S: serde::de::DeserializeOwned,
    F: Fn(Arc<AppState>, S) -> Fut,
    Fut: Future<Output = Result<(Vec<Message>, S), Error>>,
{
    let mut shutdown = state.shutdown_watch();
    let mut subscription: Option<S> = None;
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(msg)) = incoming else { break };
                match msg {
                    Message::Text(text) => {
                        let Ok(next) = serde_json::from_str::<S>(&text) else {
                            continue;
                        };
                        subscription = Some(next);
                    }
                    Message::Close(_) => break,
                    _ => continue,
                }
            }
            update = changed.changed() => {
                if update.is_err() {
                    break;
                }
            }
            // The only change to the shutdown signal is false → true.
            _ = shutdown.changed() => break,
        }
        let Some(current) = subscription.take() else {
            continue;
        };
        changed.borrow_and_update();
        let Ok((frames, next)) = read(Arc::clone(&state), current).await else {
            break;
        };
        subscription = Some(next);
        for frame in frames {
            if socket.send(frame).await.is_err() {
                return;
            }
        }
    }
}

pub(super) fn text_frame<T: serde::Serialize>(value: &T) -> Result<Message, Error> {
    let text = serde_json::to_string(value).map_err(|e| Error::internal(e.to_string()))?;
    Ok(Message::Text(text.into()))
}

/// Returns the messages that the client of a subscription lacks, and the
/// subscription with the positions after those messages.
async fn read(
    state: Arc<AppState>,
    mut subscription: Subscription,
) -> Result<(Vec<Message>, Subscription), Error> {
    with_conn(state.pool(), move |conn| {
        let positions = &mut subscription.positions;
        let mut left: BTreeMap<ChangeNumber, u64> = std::mem::take(positions);
        let mut messages = Vec::new();
        for change in changes_matching(conn, subscription.query.clone())? {
            let from = match (left.remove(&change.id), subscription.start) {
                (Some(next), _) => next,
                (None, Start::Log) => 0,
                (None, Start::Projection) => {
                    positions.insert(change.id, change.entries_folded);
                    messages.push(StreamMessage::Projection(change));
                    continue;
                }
            };
            positions.insert(change.id, from);
            if from >= change.entries_folded {
                continue;
            }
            for entry in review::entries_from(conn, change.id, from, None)? {
                positions.insert(change.id, entry.position + 1);
                messages.push(StreamMessage::Entry(entry));
            }
        }
        messages.extend(left.into_keys().map(StreamMessage::Left));
        let frames = messages.iter().map(text_frame).collect::<Result<_, _>>()?;
        Ok((frames, subscription))
    })
    .await
}
