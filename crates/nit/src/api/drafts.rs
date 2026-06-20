//! Draft endpoints (reviewer side): create, edit, and delete line-comment drafts.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::db;

use super::types;
use super::views;
use super::{AppJson, AppPath, AppState, Error, blocking};
use super::{change_or_404, snapshot_line_text, validate_anchor};

pub(super) async fn create_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::NewDraft>,
) -> Result<Json<types::Draft>, Error> {
    let entry = change_or_404(&state, id)?;
    blocking(move || {
        let conn = state.open_db()?;
        let proj = entry.read();
        let rev = proj
            .revision(req.revision)
            .ok_or_else(|| Error::bad_request(format!("revision {} not found", req.revision)))?;
        let (side, range) = validate_anchor(req.side, req.file.as_deref(), req.line, req.range)?;
        let resolution_only = req.thread_id.is_some() && req.resolved.is_some();
        if req.body.trim().is_empty() && !resolution_only {
            return Err(Error::bad_request(
                "a draft needs a body, or a thread_id with a resolution decision",
            ));
        }
        let thread_id = match req.thread_id {
            Some(tid) => {
                if proj.thread(tid).is_none() {
                    return Err(Error::bad_request("thread not found on this change"));
                }
                Some(tid)
            }
            None => None,
        };
        let git_dir = state
            .repo_state(proj.repo_id)
            .ok_or_else(|| Error::internal("repo not loaded"))?
            .git_dir();
        let line_text = snapshot_line_text(&git_dir, rev, req.file.as_deref(), req.line, side);
        drop(proj);
        let draft_id = state.alloc_id();
        let row = db::insert_draft(
            &conn,
            draft_id,
            &db::NewDraft {
                change_id: id,
                revision: req.revision,
                thread_id,
                file: req.file.as_deref(),
                line: req.line,
                side,
                range,
                line_text: line_text.as_deref(),
                body: &req.body,
                resolved: req.resolved,
            },
            &db::now_rfc3339(),
        )?;
        Ok(Json(views::draft_view(&row, id)))
    })
    .await
}

pub(super) async fn edit_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::EditDraft>,
) -> Result<Json<types::Draft>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        db::update_draft(&conn, id, &req.body, req.resolved, &db::now_rfc3339())?;
        let updated = db::get_draft(&conn, id)?
            .ok_or_else(|| Error::not_found(format!("draft {id} not found")))?;
        Ok(Json(views::draft_view(&updated, updated.change_id)))
    })
    .await
}

pub(super) async fn delete_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<StatusCode, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        if db::get_draft(&conn, id)?.is_none() {
            return Err(Error::not_found(format!("draft {id} not found")));
        }
        db::delete_draft(&conn, id)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
