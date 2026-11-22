//! HTTP API, axum 0.8.
//!
//! The wire shapes are the shared [`nit_types`] crate (the contract,
//! golden rule 4).
//!
//! - [`diff`]: diff JSON rendering and line-text snapshots.
//! - [`views`]: the per-change folds + drafts → wire shapes.
//! - [`state`]: the append primitive, the per-repo appended watch, and the
//!   errors.
//!
//! All rusqlite/git2 work runs off the async runtime; database work goes
//! through a pooled connection ([`state::with_conn`]). A read replays the
//! change from its log. An append replays it inside its own write
//! transaction ([`state::Appends`]).
//! Merged detection runs in a background timer (`timer::run_lifecycle_timer`).

pub mod diff;
mod port;
mod position;
pub mod rebase;
pub mod state;
pub mod views;

mod attachments;
mod changes;
mod comments;
mod drafts;
mod gc;
mod history;
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

use nit_types::domain::Anchor;
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::ChangeVersion;
use nit_types::domain::CommentInput;
use nit_types::health::Health;

pub use attachments::Collected;
pub use gc::{collect_attachments, collect_changes};
pub use state::{AppJson, AppPath, AppQuery, AppState, Error, with_conn};
use state::{Appends, append};
pub use timer::sweep_once;

/// Static UI serving is layered on top in [`app`].
pub fn router(state: Arc<AppState>) -> Router {
    Router::from(routes().with_state(state))
}

/// The `OpenAPI` document for every `/api` route.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().split_for_parts().1
}

// A schema that only a query parameter names is not collected from the
// routes, so it is listed here.
#[derive(utoipa::OpenApi)]
#[openapi(
    components(schemas(
        nit_types::error::ApiError,
        nit_types::domain::Tag,
        nit_types::domain::Whitespace,
    )),
    modifiers(&ErrorEnvelope)
)]
struct ApiDoc;

/// Documents the `{"error": "..."}` envelope as every operation's default
/// response, since every non-2xx response carries it.
struct ErrorEnvelope;

impl utoipa::Modify for ErrorEnvelope {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let error = utoipa::openapi::ResponseBuilder::new()
            .description("The error envelope")
            .content(
                "application/json",
                utoipa::openapi::ContentBuilder::new()
                    .schema(Some(utoipa::openapi::Ref::from_schema_name("ApiError")))
                    .build(),
            )
            .build();
        for item in openapi.paths.paths.values_mut() {
            for operation in [
                &mut item.get,
                &mut item.put,
                &mut item.post,
                &mut item.delete,
                &mut item.patch,
            ]
            .into_iter()
            .flatten()
            {
                operation
                    .responses
                    .responses
                    .insert("default".to_string(), error.clone().into());
            }
        }
    }
}

fn routes() -> utoipa_axum::router::OpenApiRouter<Arc<AppState>> {
    use utoipa_axum::routes;
    utoipa_axum::router::OpenApiRouter::with_openapi(<ApiDoc as utoipa::OpenApi>::openapi())
        .routes(routes!(health))
        .routes(routes!(attachments::upload))
        .routes(routes!(attachments::download))
        .routes(routes!(attachments::attach))
        .routes(routes!(gc::gc))
        .routes(routes!(repos::list_repos, repos::create_repo))
        .routes(routes!(
            repos::get_repo,
            repos::relocate_repo,
            repos::remove_repo
        ))
        .routes(routes!(repos::set_canonical_ref))
        .routes(routes!(push::push))
        .routes(routes!(reviews::submit))
        .routes(routes!(changes::list_changes))
        .routes(routes!(changes::list_tags))
        .routes(routes!(history::repo_history))
        .routes(routes!(changes::get_change_detail))
        .routes(routes!(changes::get_change_log))
        .routes(routes!(changes::revision_diff))
        .routes(routes!(changes::revision_lines))
        .routes(routes!(changes::ported_comments))
        .routes(routes!(changes::get_change_drafts, drafts::create_draft))
        .routes(routes!(comments::create_comment))
        .routes(routes!(
            reviews::set_draft_decision,
            reviews::clear_decision
        ))
        .routes(routes!(changes::tag_change))
        .routes(routes!(comments::abandon_change))
        .routes(routes!(comments::reopen_change))
        .routes(routes!(drafts::edit_draft, drafts::delete_draft))
        .routes(routes!(stream::stream))
}

/// The built web UI, compiled in when the build sets `NIT_WEB_DIST`.
#[cfg(nit_web_ui)]
pub static WEB_UI: Option<&include_dir::Dir> = Some(&include_dir::include_dir!("$NIT_WEB_DIST"));
#[cfg(not(nit_web_ui))]
pub static WEB_UI: Option<&include_dir::Dir> = None;

/// Unknown `/api/*` paths stay JSON 404s.
pub fn app(state: Arc<AppState>, web_ui: Option<&'static include_dir::Dir<'static>>) -> Router {
    let api = router(state).method_not_allowed_fallback(|| async {
        Error {
            status: StatusCode::METHOD_NOT_ALLOWED,
            message: "method not allowed".to_string(),
        }
    });
    api.fallback(move |req: axum::extract::Request| async move {
        let path = req.uri().path();
        if path == "/api" || path.starts_with("/api/") {
            return Error::not_found(format!("no such endpoint: {path}")).into_response();
        }
        // A client-side route names no file, so it gets index.html.
        let Some(file) = web_ui.and_then(|ui| {
            ui.get_file(path.trim_start_matches('/'))
                .or_else(|| ui.get_file("index.html"))
        }) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let mime = mime_guess::from_path(file.path()).first_or_octet_stream();
        (
            [
                (axum::http::header::CONTENT_TYPE, mime.to_string()),
                // index.html is unhashed and points at the rest, so a short
                // cap means a redeployed UI is picked up promptly.
                (axum::http::header::CACHE_CONTROL, "max-age=60".to_string()),
            ],
            file.contents(),
        )
            .into_response()
    })
}

/// Runs the background lifecycle timer alongside the server.
///
/// # Errors
///
/// When the database can't be loaded or accepting connections fails.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    db_path: PathBuf,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let addr = listener.local_addr()?;
    let state = AppState::load(db_path).await?;
    tracing::info!("listening on http://{addr}");
    let timer = tokio::spawn(timer::run_lifecycle_timer(state.clone()));
    let res = serve_on_state(listener, state, WEB_UI, shutdown).await;
    timer.abort();
    res
}

/// Serves `app` from a caller-owned [`AppState`].
///
/// Runs until `shutdown` resolves. Unlike [`serve_on`] it spawns **no
/// timer**. The only sweeps are the ones that the owner triggers through
/// [`sweep_once`]. Thus no sweep runs between the setup of an in-process
/// test and a later assertion.
///
/// # Errors
///
/// When accepting connections fails.
pub async fn serve_on_state(
    listener: tokio::net::TcpListener,
    state: Arc<AppState>,
    web_ui: Option<&'static include_dir::Dir<'static>>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let st = state.clone();
    let shutdown = async move {
        shutdown.await;
        st.begin_shutdown();
    };
    axum::serve(listener, app(state, web_ui))
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

/// Replays the change from its log, or 404s.
fn change_or_404(
    conn: &rusqlite::Connection,
    change_number: ChangeNumber,
) -> Result<ChangeProjection, Error> {
    crate::review::load(conn, change_number)?
        .ok_or_else(|| Error::not_found(format!("change {change_number} not found")))
}

fn canonical_git_dir(raw: &str) -> Result<String, Error> {
    Ok(std::fs::canonicalize(raw)
        .map_err(|e| Error::bad_request(format!("cannot resolve git dir {raw}: {e}")))?
        .to_str()
        .ok_or_else(|| Error::bad_request("git dir is not valid UTF-8"))?
        .to_string())
}

#[utoipa::path(get, path = "/api/health", responses((status = 200, body = Health)))]
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".to_string(),
        version: crate::VERSION.to_string(),
    })
}

/// The merged-history window for the change graph.
///
/// A fixed depth. `pub` so the HTTP truncation test can build exactly this
/// many commits.
pub const MERGED_WINDOW: u64 = 5;

/// The synthetic commit-message file is the route's own rule. The base of
/// a revision is its parent commit, and the parent commit contains no
/// commit message of this change. So that file has no base version.
fn new_thread(
    change: &ChangeProjection,
    anchor: Anchor,
    body: String,
    resolved: Option<bool>,
) -> Result<CommentInput, Error> {
    let number = anchor.revision();
    if change.revision(number).is_none() {
        return Err(Error::bad_request(format!(
            "revision {number} is absent from change {}",
            change.id
        )));
    }
    if let Anchor::File {
        version: ChangeVersion::Base(_),
        path,
        ..
    } = &anchor
        && path == diff::COMMIT_MSG_PATH
    {
        return Err(Error::bad_request(format!("{path} has no base version")));
    }
    let (revision, anchor) = anchor.into_log();
    CommentInput::open(revision, anchor, body, resolved)
        .map_err(|e| Error::bad_request(e.to_string()))
}

fn reply(
    change: &ChangeProjection,
    thread_id: u64,
    body: String,
    resolved: Option<bool>,
) -> Result<CommentInput, Error> {
    if change.thread(thread_id).is_none() {
        return Err(Error::bad_request(format!(
            "thread {thread_id} is absent from change {}",
            change.id
        )));
    }
    CommentInput::reply(thread_id, body, resolved).map_err(|e| Error::bad_request(e.to_string()))
}
