//! Agent endpoints: post a comment, and abandon/reopen a change.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use rusqlite::Connection;

use crate::enums::LifecycleAction;
use crate::review::{self, CommentInput, Lifecycle};

use super::types;
use super::views;
use super::{AppJson, AppPath, AppState, ChangeEntry, Error, append_to_change, with_conn};
use super::{change_detail_json, change_or_404, map_busy, snapshot_line_text, validate_anchor};

pub(super) async fn create_comment(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::NewComment>,
) -> Result<Json<types::Thread>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let resolution_only = req.thread_id.is_some() && req.resolved.is_some();
        if req.body.trim().is_empty() && !resolution_only {
            return Err(Error::bad_request("an agent comment needs a body"));
        }
        let comment = {
            let proj = entry.read();
            if let Some(tid) = req.thread_id {
                if proj.thread(tid).is_none() {
                    return Err(Error::bad_request("thread not found on this change"));
                }
                CommentInput {
                    thread_id: Some(tid),
                    revision: None,
                    file: None,
                    line: None,
                    side: None,
                    range: None,
                    line_text: None,
                    body: req.body.clone(),
                    resolved: req.resolved,
                }
            } else {
                let (side, range) =
                    validate_anchor(req.side, req.file.as_deref(), req.line, req.range)?;
                let revision = match req.revision {
                    Some(r) => r,
                    None => {
                        proj.latest_revision()
                            .ok_or_else(|| {
                                Error::bad_request(format!("change {id} has no revisions"))
                            })?
                            .number
                    }
                };
                let rev = proj
                    .revision(revision)
                    .ok_or_else(|| Error::bad_request(format!("revision {revision} not found")))?;
                let git_dir = state.git_dir(proj.repo_id)?;
                let line_text =
                    snapshot_line_text(&git_dir, rev, req.file.as_deref(), req.line, side);
                CommentInput {
                    thread_id: None,
                    revision: Some(revision),
                    file: req.file.clone(),
                    line: req.line,
                    side: Some(side),
                    range,
                    line_text,
                    body: req.body.clone(),
                    resolved: req.resolved,
                }
            }
        };
        let new = review::EntryPayload::Comment(comment);
        // A new thread's id is minted during the append, so read it back here.
        let applied = append_to_change(conn, &entry, id, vec![new]).map_err(map_busy)?;
        let thread_id = match applied.first().map(|e| &e.payload) {
            Some(review::EntryPayload::Comment(c)) => c.thread_id,
            _ => None,
        }
        .ok_or_else(|| Error::internal("comment append minted no thread"))?;
        let proj = entry.read();
        let thread = proj
            .thread(thread_id)
            .ok_or_else(|| Error::internal("thread vanished after fold"))?;
        Ok(Json(views::thread_view(thread, id)))
    })
    .await
}

/// Append a guarded lifecycle entry (a no-op unless `guard` holds for the
/// current state) then rebuild the change detail. Shared by abandon/reopen.
fn set_lifecycle(
    conn: &mut Connection,
    entry: &ChangeEntry,
    id: u64,
    guard: fn(&Lifecycle) -> bool,
    action: LifecycleAction,
    message: Option<String>,
) -> Result<Json<types::ChangeDetail>, Error> {
    if guard(&entry.read().lifecycle) {
        let new = review::EntryPayload::lifecycle(action, None, message);
        append_to_change(conn, entry, id, vec![new]).map_err(map_busy)?;
    }
    change_detail_json(conn, entry)
}

/// `POST /api/changes/{id}/abandon` — mark a live change abandoned
/// (`nit abandon`): a reviewer/agent judgment, never automatic. Optional
/// `message` records a reason. A no-op on an already-terminal change.
pub(super) async fn abandon_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::AbandonRequest>,
) -> Result<Json<types::ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        set_lifecycle(
            conn,
            &entry,
            id,
            |l| matches!(l, Lifecycle::Active),
            LifecycleAction::Abandoned,
            req.message,
        )
    })
    .await
}

/// `POST /api/changes/{id}/reopen` — clear an abandoned change back to its
/// retained verdict status (`nit reopen`).
pub(super) async fn reopen_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        set_lifecycle(
            conn,
            &entry,
            id,
            |l| matches!(l, Lifecycle::Abandoned),
            LifecycleAction::Reopened,
            None,
        )
    })
    .await
}
