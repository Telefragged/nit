//! Author endpoints: post a comment, and abandon/reopen a change.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use rusqlite::Connection;

use nit_types::changes::{AbandonRequest, ChangeDetail};
use nit_types::comments::NewComment;
use nit_types::domain::ChangeNumber;
use nit_types::domain::LifecycleAction;
use nit_types::domain::LogPayload;
use nit_types::domain::ThreadProjection;
use nit_types::domain::{Anchor, LogAnchor};

use nit_types::domain::Lifecycle;

use super::views;
use super::{AppJson, AppPath, AppState, Error, append, with_conn};
use super::{change_or_404, new_thread, reply};

#[utoipa::path(post, path = "/api/changes/{id}/comments", params(("id" = ChangeNumber, Path)), request_body = NewComment, responses((status = 200, body = ThreadProjection)))]
pub(super) async fn create_comment(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<NewComment>,
) -> Result<Json<ThreadProjection>, Error> {
    with_conn(state.pool(), move |conn| {
        // A thread or a revision, once it exists, never goes away. So the
        // decision holds at the commit, and its git read runs before the
        // write transaction.
        let change = change_or_404(conn, id)?;
        let comment = if let Some(tid) = req.thread_id {
            reply(&change, tid, req.body, req.resolved)?
        } else {
            let number = match req.revision {
                Some(r) => r,
                None => {
                    change
                        .latest_revision()
                        .ok_or_else(|| {
                            Error::bad_request(format!("change {id} has zero revisions"))
                        })?
                        .number
                }
            };
            let anchor = Anchor::from_log(number, req.anchor.unwrap_or(LogAnchor::Change));
            new_thread(&change, anchor, req.body, req.resolved)?
        };
        let appended = append(&state, conn, id, |_, _| {
            Ok(vec![LogPayload::Comment(comment)])
        })?;
        // A new thread's id is minted during the append, so read it back here.
        let thread_id = match appended.entries.first().map(|e| &e.payload) {
            Some(LogPayload::Comment(c)) => c.thread_id(),
            _ => None,
        }
        .ok_or_else(|| Error::internal("comment append minted no thread"))?;
        let thread = appended
            .change
            .thread(thread_id)
            .ok_or_else(|| Error::internal("thread vanished after fold"))?;
        Ok(Json(thread.clone()))
    })
    .await
}

/// Appends a guarded lifecycle entry, then rebuilds the change detail.
///
/// A no-op unless `guard` holds for the current state. Shared by
/// abandon/reopen.
fn set_lifecycle(
    state: &AppState,
    conn: &mut Connection,
    id: ChangeNumber,
    guard: fn(&Lifecycle) -> bool,
    action: LifecycleAction,
    message: Option<String>,
) -> Result<Json<ChangeDetail>, Error> {
    let appended = append(state, conn, id, |change, _| {
        Ok(if guard(&change.lifecycle) {
            vec![LogPayload::lifecycle(action, None, message)]
        } else {
            Vec::new()
        })
    })?;
    Ok(Json(views::build_change_detail(conn, &appended.change)?))
}

/// `POST /api/changes/{id}/abandon` — marks a live change abandoned.
///
/// `nit abandon`: a reviewer or author judgment. Optional `message`
/// records a reason. A no-op on an already-terminal change.
#[utoipa::path(post, path = "/api/changes/{id}/abandon", params(("id" = ChangeNumber, Path)), request_body = AbandonRequest, responses((status = 200, body = ChangeDetail)))]
pub(super) async fn abandon_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<AbandonRequest>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        set_lifecycle(
            &state,
            conn,
            id,
            |l| matches!(l, Lifecycle::Active),
            LifecycleAction::Abandoned,
            req.message,
        )
    })
    .await
}

/// `POST /api/changes/{id}/reopen` — reopens an abandoned change.
///
/// Clears it back to its retained verdict status (`nit reopen`).
#[utoipa::path(post, path = "/api/changes/{id}/reopen", params(("id" = ChangeNumber, Path)), responses((status = 200, body = ChangeDetail)))]
pub(super) async fn reopen_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        set_lifecycle(
            &state,
            conn,
            id,
            |l| matches!(l, Lifecycle::Abandoned),
            LifecycleAction::Reopened,
            None,
        )
    })
    .await
}
