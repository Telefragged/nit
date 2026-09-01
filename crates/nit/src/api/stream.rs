//! Events (WS `/api/stream`): the client-driven change stream, by change
//! or by tag.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

use nit_types::domain::ChangeNumber;
use nit_types::domain::LogEntry;
use nit_types::domain::Tags;
use nit_types::events::{ClientMessage, StreamMessage};

use crate::db;
use nit_types::domain::ChangeProjection;

use crate::review;

use super::state::Published;
use super::{AppState, with_conn};

/// `WS /api/stream?repo={id}` — the client-driven change stream.
///
/// The `repo` query is accepted for symmetry and ignored; the server keys
/// purely on the subscribed change numbers.
pub(super) async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// What one socket follows.
///
/// `watermark` maps each subscribed change to the next position the socket
/// sends. `tagged` is the socket's one tag subscription.
#[derive(Default)]
struct Following {
    watermark: HashMap<ChangeNumber, u64>,
    tagged: Option<TagWatch>,
}

/// The tag subscription: every change in `repo` that has `tags`.
struct TagWatch {
    repo: u64,
    tags: Tags,
    /// The highest sequence the socket has sent for this subscription. The
    /// socket sends only entries above it, so it never sends one twice.
    after: u64,
}

impl Following {
    /// Sends the change's new entries from `position` on.
    fn follow_change(&mut self, change_number: ChangeNumber, position: u64) {
        self.watermark.insert(change_number, position);
    }

    /// Sends the new entries of every change in `repo` that has `tags`,
    /// with `sequence > after`. Replaces an earlier tag subscription.
    fn follow_tags(&mut self, repo: u64, tags: Tags, after: u64) {
        self.tagged = Some(TagWatch { repo, tags, after });
    }

    /// Whether the socket sends this new entry.
    ///
    /// Both checks always run, because the tag check must update its
    /// `after` even when the change subscription already sends the entry.
    fn forwards(&mut self, published: &Published) -> bool {
        let entry = &published.entry;
        let by_change = self
            .watermark
            .get(&entry.change_number)
            .is_some_and(|&mark| entry.position >= mark);
        let by_tags = self
            .tagged
            .as_mut()
            .is_some_and(|watch| watch.forwards(published));
        by_change | by_tags
    }
}

impl TagWatch {
    /// Whether this subscription sends the entry. If so, moves `after` up
    /// to it.
    fn forwards(&mut self, published: &Published) -> bool {
        let entry = &published.entry;
        let forwarded = entry.sequence > self.after
            && self.repo == published.repo_id
            && published.tags.carries_all(&self.tags);
        if forwarded {
            self.after = entry.sequence;
        }
        forwarded
    }
}

/// Drives one follower's socket.
///
/// It holds one receiver on the server's event channel for its whole life,
/// so every subscribe is armed before it reads its backlog (a `[from, head)`
/// replay, a `ChangeProjection`, or the log past a sequence). An entry
/// written during that read arrives twice, once in the read and once on
/// the channel, and the watermark drops the second copy. An overflowed
/// receiver closes the socket — the client reconnects and re-reads the
/// log.
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut events = state.subscribe();
    let mut following = Following::default();
    let mut shutdown = state.shutdown_watch();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(msg)) = incoming else { break };
                match msg {
                    Message::Text(text) => {
                        let Ok(client) = serde_json::from_str::<ClientMessage>(&text) else {
                            continue;
                        };
                        if apply_client_msg(&mut socket, &state, &mut following, client)
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
                let Ok(published) = item else { break };
                if !following.forwards(&published) {
                    continue;
                }
                if send(&mut socket, &StreamMessage::Entry(published.entry)).await.is_err() {
                    break;
                }
            }
            // The only change to the shutdown signal is false → true.
            _ = shutdown.changed() => break,
        }
    }
}

/// Applies one client message; `Err(())` means the socket should close.
async fn apply_client_msg(
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    following: &mut Following,
    client: ClientMessage,
) -> Result<(), ()> {
    match client {
        ClientMessage::Subscribe(map) => {
            let cursors = map
                .iter()
                .filter_map(|(id, from)| Some((id.parse::<ChangeNumber>().ok()?, *from)))
                .collect();
            for (change_number, next, backlog) in read_backlogs(state, cursors).await {
                following.follow_change(change_number, next);
                for e in backlog {
                    send(socket, &StreamMessage::Entry(e)).await?;
                }
            }
        }
        ClientMessage::SubscribeProjection(ids) => {
            for (change_number, proj) in read_projections(state, ids).await {
                // The projection's `entries_folded` is the high-water mark, so an
                // append that lands after it rides the channel and is deduped
                // there: the projection and its live tail neither gap nor
                // double.
                following.follow_change(change_number, proj.entries_folded);
                send(socket, &StreamMessage::Projection(proj)).await?;
            }
        }
        ClientMessage::SubscribeTagged { repo, tags, after } => {
            let backlog = read_log_after(state, repo, tags.clone(), after).await;
            let after = backlog.last().map_or(after, |e| e.sequence);
            following.follow_tags(repo, tags, after);
            for e in backlog {
                send(socket, &StreamMessage::Entry(e)).await?;
            }
        }
    }
    Ok(())
}

async fn send(socket: &mut WebSocket, msg: &StreamMessage) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// Each cursor's log slice `[from, head)` as tagged entries.
///
/// With the position that slice ends at, read over one borrowed connection — a
/// subscribe carries a whole chain, so the frames it answers with are sent
/// after the read rather than between two of them. A change left out of the
/// result is left unsubscribed: it does not exist, or the read failed and
/// the follower re-reads on reconnect.
async fn read_backlogs(
    state: &Arc<AppState>,
    cursors: Vec<(ChangeNumber, u64)>,
) -> Vec<(ChangeNumber, u64, Vec<LogEntry>)> {
    with_conn(state.pool(), move |conn| {
        let mut out = Vec::with_capacity(cursors.len());
        for (change_number, from) in cursors {
            // Existence is a row read: cursor mode replays the log itself and
            // never touches the fold.
            if db::get_change(conn, change_number)?.is_none() {
                continue;
            }
            let rows = db::log_entries(conn, change_number, from, None)?;
            let entries = rows
                .iter()
                .map(|r| review::entry_from_row(change_number, r))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let next = entries.last().map_or(from, |e| e.position + 1);
            out.push((change_number, next, entries));
        }
        Ok(out)
    })
    .await
    .unwrap_or_default()
}

/// The stored entries with `sequence > after` of every change in `repo`
/// that has `tags`.
///
/// Empty when the read fails. The client then gets no replay, and it reads
/// the log again the next time an entry arrives.
async fn read_log_after(state: &Arc<AppState>, repo: u64, tags: Tags, after: u64) -> Vec<LogEntry> {
    with_conn(state.pool(), move |conn| {
        let filter = db::ChangeFilter {
            tags,
            ..db::ChangeFilter::default()
        };
        Ok(review::entries_between(
            conn,
            repo,
            &filter,
            Some(after),
            None,
        )?)
    })
    .await
    .unwrap_or_default()
}

/// Each change's folded projection, cloned from under its read lock.
///
/// No guard is held across a send — the one place a fold is resolved
/// without a connection already in hand, so it borrows one for the whole
/// batch.
async fn read_projections(
    state: &Arc<AppState>,
    ids: Vec<ChangeNumber>,
) -> Vec<(ChangeNumber, ChangeProjection)> {
    let st = state.clone();
    with_conn(state.pool(), move |conn| {
        let mut out = Vec::with_capacity(ids.len());
        for change_number in ids {
            if let Some(entry) = st.change(conn, change_number)? {
                out.push((change_number, entry.read().clone()));
            }
        }
        Ok(out)
    })
    .await
    .unwrap_or_default()
}
