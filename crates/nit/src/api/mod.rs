//! HTTP API: every endpoint of `docs/api.md` (the contract), axum 0.8.
//!
//! - [`types`] — the wire-shape mirror of docs/api.md (golden rule 4).
//! - [`diff`] — diff JSON rendering and line-text snapshots.
//! - [`views`] — the per-change folds + chain derivation → wire shapes.
//! - [`state`] — the in-memory fold, the append primitive, errors.
//!
//! All rusqlite/git2 work runs in `spawn_blocking`; every appender to one
//! change serializes through its projection write lock and folds in lock-step
//! (docs/data-model.md "Concurrency"). A chain owns nothing — it is derived at
//! read time. Merged/abandoned detection runs in a background timer
//! ([`timer::run_lifecycle_timer`]); there are no read-time scans.

pub mod diff;
pub mod rebase;
pub mod state;
pub mod types;
pub mod views;

mod chains;
mod changes;
mod comments;
mod drafts;
mod push;
mod repos;
mod reviews;
mod stream;
mod timer;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post, put};
use git2::Repository;
use serde::Deserialize;

use crate::enums::Side;
use crate::review;

pub use state::{
    AppJson, AppPath, AppQuery, AppState, ChangeEntry, Error, append_to_change,
    append_to_change_with, blocking,
};

/// The `/api` router. Static UI serving is layered on top in [`app`].
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route(
            "/api/repos",
            get(repos::list_repos).post(repos::create_repo),
        )
        .route(
            "/api/repos/{id}",
            get(repos::get_repo).patch(repos::relocate_repo),
        )
        .route("/api/repos/{id}/graph", get(chains::repo_graph))
        .route("/api/push", post(push::push))
        .route("/api/chains", get(chains::list_chains))
        .route("/api/chains/{id}", get(chains::get_chain))
        .route("/api/chains/{id}/log", get(chains::chain_log))
        .route("/api/chains/{id}/submit", post(reviews::submit_chain))
        .route("/api/changes/{id}", get(changes::get_change_detail))
        .route(
            "/api/changes/{id}/revisions/{n}/diff",
            get(changes::revision_diff),
        )
        .route("/api/changes/{id}/drafts", post(drafts::create_draft))
        .route("/api/changes/{id}/comments", post(comments::create_comment))
        .route(
            "/api/changes/{id}/decision",
            put(reviews::stage_decision).delete(reviews::clear_decision),
        )
        .route("/api/changes/{id}/abandon", post(comments::abandon_change))
        .route("/api/changes/{id}/reopen", post(comments::reopen_change))
        .route(
            "/api/drafts/{id}",
            patch(drafts::edit_draft).delete(drafts::delete_draft),
        )
        .route("/api/stream", get(stream::stream))
        .with_state(state)
}

/// Full application: `/api` plus the built web UI with an `index.html` SPA
/// fallback. Unknown `/api/*` paths stay JSON 404s.
pub fn app(state: Arc<AppState>, web_dist: Option<PathBuf>) -> Router {
    let api = router(state).method_not_allowed_fallback(|| async {
        Error {
            status: StatusCode::METHOD_NOT_ALLOWED,
            message: "method not allowed".to_string(),
        }
    });
    let spa = web_dist.map(|dist| {
        tower_http::services::ServeDir::new(&dist).fallback(tower_http::services::ServeFile::new(
            dist.join("index.html"),
        ))
    });
    api.fallback(move |req: axum::extract::Request| {
        let spa = spa.clone();
        async move {
            let path = req.uri().path();
            if path == "/api" || path.starts_with("/api/") {
                return Error::not_found(format!("no such endpoint: {path}")).into_response();
            }
            match spa {
                Some(spa) => match tower::ServiceExt::oneshot(spa, req).await {
                    Ok(resp) => resp.into_response(),
                    Err(infallible) => match infallible {},
                },
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    })
}

/// Serve `app` on an already-bound listener until `shutdown` resolves, running
/// the background lifecycle timer alongside.
///
/// # Errors
/// When the database can't be loaded or accepting connections fails.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    db_path: PathBuf,
    web_dist: Option<PathBuf>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let addr = listener.local_addr()?;
    let state = AppState::load(db_path)?;
    tracing::info!("listening on http://{addr}");
    let timer = tokio::spawn(timer::run_lifecycle_timer(state.clone()));
    let st = state.clone();
    let shutdown = async move {
        shutdown.await;
        st.begin_shutdown();
    };
    axum::serve(listener, app(state, web_dist))
        .with_graceful_shutdown(shutdown)
        .await?;
    timer.abort();
    Ok(())
}

// ---------------------------------------------------------------------------
// Routing helpers

fn change_or_404(state: &Arc<AppState>, change_id: u64) -> Result<Arc<ChangeEntry>, Error> {
    state
        .change_entry(change_id)
        .ok_or_else(|| Error::not_found(format!("change {change_id} not found")))
}

/// The git handle for a change's repo.
fn repo_of_change(state: &Arc<AppState>, entry: &ChangeEntry) -> Result<Repository, Error> {
    let repo_id = entry.read().repo_id;
    let repo = state
        .repo_state(repo_id)
        .ok_or_else(|| Error::internal(format!("repo {repo_id} not loaded")))?;
    Repository::open(repo.git_dir())
        .map_err(|e| Error::internal(format!("cannot open the repository: {e}")))
}

/// Canonicalize a git-dir path to a UTF-8 string, or a 400.
fn canonical_git_dir(raw: &str) -> Result<String, Error> {
    Ok(std::fs::canonicalize(raw)
        .map_err(|e| Error::bad_request(format!("cannot resolve git dir {raw}: {e}")))?
        .to_str()
        .ok_or_else(|| Error::bad_request("git dir is not valid UTF-8"))?
        .to_string())
}

fn map_busy(err: anyhow::Error) -> Error {
    if state::is_sqlite_busy(&err) {
        Error::unavailable("database is busy (another change is being written) — retry shortly")
    } else {
        err.into()
    }
}

// ---------------------------------------------------------------------------
// Health

async fn health() -> Json<types::Health> {
    Json(types::Health {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// The merged-history window for the change graph (docs/api.md "Graph"): a
/// fixed depth, not a client knob. `pub` so the HTTP truncation test can build
/// exactly this many commits.
pub const MERGED_WINDOW: u64 = 5;

#[derive(Deserialize)]
struct ChainQuery {
    revision: Option<u64>,
}

/// Rebuild the `ChangeDetail` for `id` from the current view (404 if it
/// vanished) — the shared tail of the three change-detail handlers.
fn change_detail_json(
    conn: &rusqlite::Connection,
    state: &Arc<AppState>,
    entry: &ChangeEntry,
    id: u64,
) -> Result<Json<types::ChangeDetail>, Error> {
    let repo = repo_of_change(state, entry)?;
    let repo_id = entry.read().repo_id;
    let view = state.repo_view(repo_id);
    let change = view
        .change(id)
        .ok_or_else(|| Error::not_found(format!("change {id} not found")))?;
    Ok(Json(views::build_change_detail(
        conn, &repo, &view, change,
    )?))
}

/// The "Range comments" rules of docs/api.md.
fn validate_range(
    range: types::CommentRange,
    line: Option<u64>,
) -> Result<types::CommentRange, Error> {
    if line.is_none() {
        return Err(Error::bad_request("a range requires a line anchor"));
    }
    if line != Some(range.end_line) {
        return Err(Error::bad_request(
            "range.end_line must equal the comment's line",
        ));
    }
    let forward = range.start_line < range.end_line
        || (range.start_line == range.end_line && range.start_char < range.end_char);
    if range.start_line < 1 || range.end_char < 1 || !forward {
        return Err(Error::bad_request(
            "range must be non-empty and forward (docs/api.md \"Range comments\")",
        ));
    }
    Ok(range)
}

fn validate_anchor(
    side: Option<Side>,
    file: Option<&str>,
    line: Option<u64>,
    range: Option<types::CommentRange>,
) -> Result<(Side, Option<types::CommentRange>), Error> {
    let side = side.unwrap_or_default();
    if line.is_some() && file.is_none() {
        return Err(Error::bad_request("a line anchor requires a file"));
    }
    if file == Some(diff::COMMIT_MSG_PATH) && side == Side::Old {
        return Err(Error::bad_request(
            "/COMMIT_MSG has no old side — comment with side \"new\"",
        ));
    }
    let range = range.map(|r| validate_range(r, line)).transpose()?;
    Ok((side, range))
}

fn snapshot_line_text(
    git_dir: &str,
    rev: &review::RevisionProj,
    file: Option<&str>,
    line: Option<u64>,
    side: Side,
) -> Option<String> {
    match (file, line) {
        (Some(diff::COMMIT_MSG_PATH), Some(line)) => diff::nth_line(&rev.message, line),
        (Some(file), Some(line)) => {
            let sha = if side == Side::Old {
                &rev.parent_sha
            } else {
                &rev.commit_sha
            };
            Repository::open(git_dir).ok().and_then(|repo| {
                diff::commit_tree(&repo, sha).and_then(|t| diff::line_text(&repo, &t, file, line))
            })
        }
        _ => None,
    }
}
