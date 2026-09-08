//! Events (WS `/api/stream`): the client-driven change stream.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

use nit_types::domain::ChangeNumber;
use nit_types::events::{StreamMessage, Subscription};

use crate::db;
use crate::review;

use super::state::Published;
use super::{AppState, with_conn};

/// `WS /api/stream` — the client-driven change stream.
///
/// `nit_types::events::Subscription` carries the contract.
pub(super) async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// What one socket follows.
struct Watch {
    repo: Option<u64>,
    filter: db::ChangeFilter,
    /// The highest sequence the socket has sent. The socket sends only
    /// entries above it, so it never sends one twice.
    after: u64,
    /// The changes whose projection the socket has.
    announced: HashSet<ChangeNumber>,
}

impl Watch {
    /// Whether the socket sends this new entry. If so, moves `after` up to
    /// it.
    fn forwards(&mut self, published: &Published) -> bool {
        let forwarded = published.entry.sequence > self.after
            && self.repo.is_none_or(|r| r == published.change.repo_id)
            && self.filter.matches(&published.change);
        if forwarded {
            self.after = published.entry.sequence;
        }
        forwarded
    }
}

/// Drives one follower's socket.
///
/// It holds one receiver on the server's event channel for its whole life,
/// so a subscription is armed before it reads its opening frames. An entry
/// written during that read arrives twice, once in the read and once on
/// the channel, and `after` or the projection's `entries_folded` drops the
/// second copy. An overflowed receiver closes the socket — the client
/// reconnects and subscribes again.
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut events = state.subscribe();
    let mut watch: Option<Watch> = None;
    let mut shutdown = state.shutdown_watch();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(msg)) = incoming else { break };
                match msg {
                    Message::Text(text) => {
                        let Ok(subscription) = serde_json::from_str::<Subscription>(&text) else {
                            continue;
                        };
                        match subscribe(&mut socket, &state, subscription).await {
                            Ok(w) => watch = Some(w),
                            Err(()) => break,
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            item = events.recv() => {
                // Overflow (or a closed channel): this follower fell behind.
                // Close the socket so it reconnects and re-reads the gap.
                let Ok(published) = item else { break };
                let Some(w) = watch.as_mut() else { continue };
                if !w.forwards(&published) {
                    continue;
                }
                let Published { entry, change } = published;
                if w.announced.insert(entry.change_number)
                    && send(&mut socket, &StreamMessage::Projection((*change).clone())).await.is_err()
                {
                    break;
                }
                if send(&mut socket, &StreamMessage::Entry(entry)).await.is_err() {
                    break;
                }
            }
            // The only change to the shutdown signal is false → true.
            _ = shutdown.changed() => break,
        }
    }
}

/// Sends a subscription's opening frames and returns the watch that
/// follows it; `Err(())` means the socket should close.
async fn subscribe(
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    subscription: Subscription,
) -> Result<Watch, ()> {
    let frames = opening_frames(state, &subscription).await;
    let mut watch = Watch {
        repo: subscription.query.repo,
        filter: db::ChangeFilter::from(subscription.query),
        after: subscription.after.unwrap_or(0),
        announced: HashSet::new(),
    };
    for frame in frames {
        match &frame {
            StreamMessage::Projection(proj) => {
                watch.announced.insert(proj.id);
            }
            StreamMessage::Entry(entry) => watch.after = watch.after.max(entry.sequence),
        }
        send(socket, &frame).await?;
    }
    Ok(watch)
}

async fn send(socket: &mut WebSocket, msg: &StreamMessage) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// What a subscription sends first: the picked projections, then the
/// stored entries past `after` when given.
///
/// Empty when the read fails. The socket then sends no opening frame, and
/// it announces each picked change when it first meets one live.
async fn opening_frames(state: &Arc<AppState>, subscription: &Subscription) -> Vec<StreamMessage> {
    let st = state.clone();
    let query = subscription.query.clone();
    let after = subscription.after;
    with_conn(state.pool(), move |conn| {
        let repo_ids = st.repo_ids_matching(query.repo);
        let mut frames: Vec<StreamMessage> = st
            .changes_matching(conn, query.clone())?
            .into_iter()
            .map(StreamMessage::Projection)
            .collect();
        if let Some(after) = after {
            let filter = db::ChangeFilter::from(query);
            for repo_id in repo_ids {
                let entries = review::entries_between(conn, repo_id, &filter, Some(after), None)?;
                frames.extend(entries.into_iter().map(StreamMessage::Entry));
            }
        }
        Ok(frames)
    })
    .await
    .unwrap_or_default()
}
