//! Draft endpoints (reviewer side): create, edit, and delete line-comment drafts.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use nit_types::comments::{EditDraft, NewDraft};
use nit_types::domain::ChangeNumber;
use nit_types::domain::Draft;

use crate::db;

use super::{AppJson, AppPath, AppState, Error, with_conn};
use super::{change_or_404, new_thread, reply};

#[utoipa::path(post, path = "/api/changes/{id}/drafts", params(("id" = ChangeNumber, Path)), request_body = NewDraft, responses((status = 200, body = Draft)))]
pub(super) async fn create_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<NewDraft>,
) -> Result<Json<Draft>, Error> {
    with_conn(state.pool(), move |conn| {
        let proj = change_or_404(conn, id)?;
        let comment = match req {
            NewDraft::Thread {
                anchor,
                body,
                resolved,
            } => new_thread(&proj, anchor, body, resolved)?,
            NewDraft::Reply {
                thread_id,
                body,
                resolved,
            } => reply(&proj, thread_id, body, resolved)?,
        };
        let draft_id = state.alloc_id();
        let draft = db::write(conn, |tx| {
            db::insert_draft(tx, draft_id, id, comment, &db::now_rfc3339())
        })?;
        Ok(Json(draft))
    })
    .await
}

#[utoipa::path(patch, path = "/api/drafts/{id}", params(("id" = u64, Path)), request_body = EditDraft, responses((status = 200, body = Draft)))]
pub(super) async fn edit_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<EditDraft>,
) -> Result<Json<Draft>, Error> {
    with_conn(state.pool(), move |conn| {
        let updated = db::write(conn, |tx| {
            db::update_draft(tx, id, &req.body, req.resolved, &db::now_rfc3339())
        })?
        .ok_or_else(|| Error::not_found(format!("draft {id} not found")))?;
        Ok(Json(updated))
    })
    .await
}

#[utoipa::path(delete, path = "/api/drafts/{id}", params(("id" = u64, Path)), responses((status = 204)))]
pub(super) async fn delete_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<StatusCode, Error> {
    with_conn(state.pool(), move |conn| {
        if !db::write(conn, |tx| db::delete_draft(tx, id))? {
            return Err(Error::not_found(format!("draft {id} not found")));
        }
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
