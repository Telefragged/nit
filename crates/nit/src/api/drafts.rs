//! Draft endpoints (reviewer side): create, edit, and delete line-comment drafts.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use nit_types::comments::{EditDraft, NewDraft};
use nit_types::domain::ChangeNumber;
use nit_types::domain::Draft;

use crate::db;

use super::views;
use super::{AppJson, AppPath, AppState, Error, with_conn};
use super::{change_or_404, new_thread_anchor};

#[utoipa::path(post, path = "/api/changes/{id}/drafts", params(("id" = ChangeNumber, Path)), request_body = NewDraft, responses((status = 200, body = Draft)))]
pub(super) async fn create_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<NewDraft>,
) -> Result<Json<Draft>, Error> {
    with_conn(state.pool(), move |conn| {
        let proj = change_or_404(conn, id)?;
        let (thread_id, anchor, body, resolved) = match req {
            NewDraft::Thread {
                anchor,
                body,
                resolved,
            } => {
                if body.trim().is_empty() {
                    return Err(Error::bad_request("a new thread needs a body"));
                }
                let anchor = new_thread_anchor(&proj, anchor)?;
                (None, anchor, body, resolved)
            }
            NewDraft::Reply {
                thread_id,
                body,
                resolved,
            } => {
                if body.trim().is_empty() && resolved.is_none() {
                    return Err(Error::bad_request(
                        "a reply needs a body or a resolution decision",
                    ));
                }
                let anchor = proj
                    .thread(thread_id)
                    .ok_or_else(|| Error::bad_request("thread not found on this change"))?
                    .anchor
                    .clone();
                (Some(thread_id), anchor, body, resolved)
            }
        };
        let draft_id = state.alloc_id();
        let new = db::NewDraft {
            change_number: id,
            thread_id,
            anchor: &anchor,
            body: &body,
            resolved,
        };
        let row = db::write(conn, |tx| {
            db::insert_draft(tx, draft_id, &new, &db::now_rfc3339())
        })?;
        Ok(Json(views::draft_view(&row)))
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
            db::update_draft(tx, id, &req.body, req.resolved, &db::now_rfc3339())?;
            db::get_draft(tx, id)
        })?
        .ok_or_else(|| Error::not_found(format!("draft {id} not found")))?;
        Ok(Json(views::draft_view(&updated)))
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
