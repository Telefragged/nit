//! HTTP API: every endpoint of `docs/api.md` (the contract), axum 0.8.
//!
//! - [`types`] — the wire-shape mirror of docs/api.md (golden rule 3).
//! - [`diff`] — diff JSON rendering and comment-anchor porting.
//! - [`views`] — db rows → wire shapes (chain state derivation included).
//! - [`state`] — per-chain locks, scan throttle/orchestration, errors.
//!
//! All rusqlite/git2 work runs in `spawn_blocking`; scans and review
//! submissions to one chain serialize through its async mutex
//! (docs/data-model.md "Concurrency").

pub mod diff;
pub mod state;
pub mod types;
pub mod views;

use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use git2::{Oid, Repository};
use rusqlite::{Connection, TransactionBehavior};
use serde::Deserialize;

use crate::db::{self, ChangeStatus};
use crate::gitscan;

pub use state::{AppJson, AppPath, AppQuery, AppState, Error, blocking, scan_chain};

/// The `/api` router. Static UI serving is layered on top in [`app`].
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/chains", post(register_chain).get(list_chains))
        .route("/api/chains/{id}", get(get_chain))
        .route("/api/chains/{id}/feedback", get(get_feedback))
        .route("/api/chains/{id}/wait", get(wait_chain))
        .route("/api/changes/{id}", get(get_change_detail))
        .route("/api/changes/{id}/revisions/{n}/diff", get(revision_diff))
        .route("/api/changes/{id}/drafts", post(create_draft))
        .route("/api/changes/{id}/reviews", post(submit_review))
        .route("/api/drafts/{id}", patch(edit_draft).delete(delete_draft))
        .route("/api/comments/{id}/resolve", post(resolve_comment))
        .route("/api/comments/{id}/unresolve", post(unresolve_comment))
        .route("/api/comments/{id}/replies", post(create_reply))
        .with_state(state)
}

/// Full application: `/api` plus the built web UI (`--web-dist` /
/// `$NIT_WEB_DIST`) with an `index.html` SPA fallback for client-side
/// routes. Without a web dist the server is API-only. Unknown `/api/*`
/// paths must stay JSON 404s (api.md "Everything under /api, JSON
/// in/out") — they never fall through to the SPA.
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

/// Serve `app` on an already-bound listener until `shutdown` resolves.
/// `public_base` (every `web_url`) comes from the listener's local addr.
///
/// # Errors
/// When accepting connections on `listener` fails.
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    db_path: PathBuf,
    web_dist: Option<PathBuf>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let addr = listener.local_addr()?;
    let state = AppState::new(db_path, format!("http://{addr}"));
    state.open_db()?; // fail fast: create/migrate before accepting requests
    tracing::info!("listening on http://{addr}");
    // Flip the state's shutdown watch the moment graceful shutdown
    // begins: axum waits for in-flight requests, so without it a parked
    // /wait long-poll would hold ctrl-c hostage for its full timeout.
    let st = state.clone();
    let shutdown = async move {
        shutdown.await;
        st.begin_shutdown();
    };
    axum::serve(listener, app(state, web_dist))
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared loaders (blocking context)

fn load_chain(conn: &Connection, id: i64) -> Result<db::Chain, Error> {
    db::get_chain(conn, id)?.ok_or_else(|| Error::not_found(format!("chain {id} not found")))
}

fn load_change(conn: &Connection, id: i64) -> Result<db::Change, Error> {
    db::get_change(conn, id)?.ok_or_else(|| Error::not_found(format!("change {id} not found")))
}

fn load_comment(conn: &Connection, id: i64) -> Result<db::Comment, Error> {
    db::get_comment(conn, id)?.ok_or_else(|| Error::not_found(format!("comment {id} not found")))
}

/// Open a chain's repository; `None` degrades reads (comments render as
/// outdated) instead of failing whole responses.
fn open_repo(conn: &Connection, chain_id: i64) -> Option<Repository> {
    let path = db::chain_repo_path(conn, chain_id).ok().flatten()?;
    Repository::open(path).ok()
}

/// Build the full Chain JSON for responses from current db state.
async fn chain_response(state: Arc<AppState>, chain_id: i64) -> Result<Json<types::Chain>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let chain = load_chain(&conn, chain_id)?;
        Ok(Json(views::build_chain(&conn, &state.public_base, &chain)?))
    })
    .await
}

// ---------------------------------------------------------------------------
// Health

async fn health() -> Json<types::Health> {
    Json(types::Health {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ---------------------------------------------------------------------------
// Chains

async fn register_chain(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<types::RegisterChain>,
) -> Result<Json<types::Chain>, Error> {
    let partial = req.partial;
    let st = state.clone();
    let chain = blocking(move || {
        let conn = st.open_db()?;
        // An *existing* chain re-registers even when git is mid-rebase:
        // scan failures then surface as last_scan_error, not HTTP errors.
        let canonical = std::fs::canonicalize(&req.repo_path).map_err(|e| {
            Error::bad_request(format!("cannot resolve repo path {}: {e}", req.repo_path))
        })?;
        let canonical = canonical
            .to_str()
            .ok_or_else(|| Error::bad_request("repo path is not valid UTF-8"))?;
        match db::find_chain_by_repo_branch(&conn, canonical, &req.branch)? {
            Some(existing) => Ok(db::get_or_create_chain(
                &conn,
                existing.repo_id,
                &req.branch,
                &req.base,
            )?),
            None => gitscan::register(&conn, FsPath::new(&req.repo_path), &req.branch, &req.base)
                .map_err(|e| Error::bad_request(format!("{e:#}"))),
        }
    })
    .await?;
    // Scan before applying partial: the flip wakes /wait long-polls, which
    // read without the chain lock. Flipping first would let a `nit ready`
    // carrying unscanned commits wake a waiter into the window between
    // the two commits, where the old all-approved change set reads
    // ready_to_merge — exactly the premature-merge state partial exists to
    // make inexpressible. Scan-then-flip is uniformly safe: waiters woken
    // by the flip see the post-scan change set, and for push --partial
    // the scanned-in pending commits already block ready_to_merge.
    scan_chain(&state, chain.id, true).await?; // push always rescans
    if let Some(partial) = partial {
        apply_partial(&state, chain.id, partial).await?;
    }
    chain_response(state, chain.id).await
}

/// Apply the sticky `partial` flag from a registration. An actual flip is
/// chain state the agent acts on, so it emits `chain_updated` and wakes
/// `/wait` long-polls exactly like a scan-emitted event; setting the
/// already-stored value does neither. Runs under the chain lock — no
/// scan or review submission races the flip (docs/data-model.md
/// "Concurrency").
async fn apply_partial(state: &Arc<AppState>, chain_id: i64, partial: bool) -> Result<(), Error> {
    let entry = state.entry(chain_id);
    let gate = entry.gate.lock().await;
    let st = state.clone();
    let flipped = blocking(move || {
        let txn = || -> anyhow::Result<bool> {
            let mut conn = st.open_db()?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let flipped = db::chain_set_partial(&tx, chain_id, partial)?;
            if flipped {
                // Deliberately no chain_touch: updated_at times the
                // branch-missing abandon window and a partial flip is not a scan.
                db::insert_event(
                    &tx,
                    chain_id,
                    "chain_updated",
                    &serde_json::json!({"chain_id": chain_id}),
                    &db::now_rfc3339(),
                )?;
            }
            tx.commit()?;
            Ok(flipped)
        };
        // Cross-chain write contention gets the same retryable 503 the
        // scan running next in this request returns (state.rs scan_chain),
        // not an opaque 500.
        txn().map_err(|err| {
            if state::is_sqlite_busy(&err) {
                Error::unavailable(
                    "database is busy (another chain is being scanned) — retry shortly",
                )
            } else {
                err.into()
            }
        })
    })
    .await?;
    drop(gate);
    if flipped {
        entry.notify.notify_waiters();
    }
    Ok(())
}

#[derive(Deserialize)]
struct ListChainsQuery {
    status: Option<String>,
}

async fn list_chains(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ListChainsQuery>,
) -> Result<Json<types::ChainList>, Error> {
    let include_closed = match q.status.as_deref() {
        None | Some("active") => false,
        Some("all") => true,
        Some(other) => {
            return Err(Error::bad_request(format!(
                "unknown status filter {other:?} (expected \"active\" or \"all\")"
            )));
        }
    };

    let st = state.clone();
    let ids: Vec<i64> = blocking(move || {
        let conn = st.open_db()?;
        Ok(db::list_chains(&conn)?.iter().map(|c| c.id).collect())
    })
    .await?;
    // Throttled scan per chain; failures are isolated into each chain's
    // last_scan_error and must not affect listing the others.
    for id in &ids {
        scan_chain(&state, *id, false).await?;
    }

    blocking(move || {
        let conn = state.open_db()?;
        let mut chains = Vec::new();
        for id in ids {
            let Some(chain) = db::get_chain(&conn, id)? else {
                continue;
            };
            if !include_closed && chain.status != db::ChainStatus::Active {
                continue;
            }
            chains.push(views::build_chain(&conn, &state.public_base, &chain)?);
        }
        Ok(Json(types::ChainList { chains }))
    })
    .await
}

async fn get_chain(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
) -> Result<Json<types::Chain>, Error> {
    let st = state.clone();
    blocking(move || {
        let conn = st.open_db()?;
        load_chain(&conn, id).map(|_| ())
    })
    .await?;
    scan_chain(&state, id, false).await?;
    chain_response(state, id).await
}

// ---------------------------------------------------------------------------
// Changes

#[derive(Deserialize)]
struct ChangeQuery {
    revision: Option<i64>,
}

async fn get_change_detail(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
    AppQuery(q): AppQuery<ChangeQuery>,
) -> Result<Json<types::ChangeDetail>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let change = load_change(&conn, id)?;
        let latest = db::latest_revision(&conn, change.id)?
            .ok_or_else(|| Error::internal(format!("change {id} has no revisions")))?;
        let requested = match q.revision {
            None => latest.number,
            Some(n) => {
                db::get_revision(&conn, change.id, n)?
                    .ok_or_else(|| Error::not_found(format!("revision {n} not found")))?
                    .number
            }
        };
        let repo = open_repo(&conn, change.chain_id);
        Ok(Json(views::build_change_detail(
            &conn,
            repo.as_ref(),
            &change,
            requested,
        )?))
    })
    .await
}

#[derive(Deserialize)]
struct DiffQuery {
    against: Option<i64>,
}

async fn revision_diff(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(i64, i64)>,
    AppQuery(q): AppQuery<DiffQuery>,
) -> Result<Json<types::Diff>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let change = load_change(&conn, id)?;
        let rev = db::get_revision(&conn, change.id, n)?
            .ok_or_else(|| Error::not_found(format!("revision {n} not found")))?;
        let repo = open_repo(&conn, change.chain_id)
            .ok_or_else(|| Error::internal("cannot open the chain's repository"))?;
        let new_tree = revision_tree(&repo, &rev)?;
        // Interdiffs also diff the two revisions' commit messages; vs
        // parent the message has no old side (against_message: None).
        let (old_tree, against_message) = match q.against {
            None => {
                let parent = repo
                    .find_commit(parse_oid(&rev.parent_sha)?)
                    .map_err(|e| Error::internal(format!("parent commit missing: {e}")))?;
                let tree = parent
                    .tree()
                    .map_err(|e| Error::internal(format!("parent tree missing: {e}")))?;
                (tree, None)
            }
            Some(m) => {
                let against = db::get_revision(&conn, change.id, m)?
                    .ok_or_else(|| Error::not_found(format!("revision {m} not found")))?;
                (revision_tree(&repo, &against)?, Some(against.message))
            }
        };
        let mut wire = diff::diff_trees(&repo, &old_tree, &new_tree)?;
        wire.files.insert(
            0,
            diff::commit_msg_file(against_message.as_deref(), &rev.message)?,
        );
        Ok(Json(wire))
    })
    .await
}

/// A revision's reviewed tree: its commit's own tree.
fn revision_tree<'r>(repo: &'r Repository, rev: &db::Revision) -> Result<git2::Tree<'r>, Error> {
    diff::commit_tree(repo, &rev.commit_sha)
        .ok_or_else(|| Error::internal(format!("revision {} tree missing", rev.number)))
}

fn parse_oid(sha: &str) -> Result<Oid, Error> {
    Oid::from_str(sha).map_err(|e| Error::internal(format!("bad sha {sha:?}: {e}")))
}

// ---------------------------------------------------------------------------
// Drafts (reviewer side)

async fn create_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
    AppJson(req): AppJson<types::NewDraft>,
) -> Result<Json<types::Comment>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let change = load_change(&conn, id)?;
        let rev = db::get_revision(&conn, change.id, req.revision)?
            .ok_or_else(|| Error::bad_request(format!("revision {} not found", req.revision)))?;
        let side = req.side.as_deref().unwrap_or("new");
        if side != "new" && side != "old" {
            return Err(Error::bad_request(format!(
                "side must be \"new\" or \"old\", got {side:?}"
            )));
        }
        if req.line.is_some() && req.file.is_none() {
            return Err(Error::bad_request("a line anchor requires a file"));
        }
        if req.file.as_deref() == Some(diff::COMMIT_MSG_PATH) && side == "old" {
            return Err(Error::bad_request(
                "/COMMIT_MSG has no old side — comment with side \"new\"",
            ));
        }
        let range = req.range.map(|r| validate_range(r, req.line)).transpose()?;
        // Thread under the root, wherever the draft pointed (like replies):
        // feedback scoping only walks one level below roots, so a comment
        // threaded under a reply would silently vanish from the agent's view.
        let parent_id = match req.parent_id {
            Some(parent_id) => {
                let parent = load_comment(&conn, parent_id)?;
                if parent.change_id != change.id {
                    return Err(Error::bad_request(
                        "parent comment belongs to a different change",
                    ));
                }
                let mut root = parent;
                while let Some(up) = root.parent_id {
                    root = load_comment(&conn, up)?;
                }
                Some(root.id)
            }
            None => None,
        };
        let line_text = match (req.file.as_deref(), req.line) {
            (Some(diff::COMMIT_MSG_PATH), Some(line)) => diff::nth_line(&rev.message, line),
            (Some(file), Some(line)) => open_repo(&conn, change.chain_id)
                .and_then(|repo| anchor_line_text(&repo, &rev, side, file, line)),
            _ => None,
        };
        let row = db::insert_comment(
            &conn,
            &db::NewComment {
                change_id: change.id,
                revision_number: rev.number,
                parent_id,
                author: "reviewer",
                file: req.file.as_deref(),
                line: req.line,
                side,
                range,
                line_text: line_text.as_deref(),
                body: &req.body,
                state: "draft",
                resolved: false,
            },
            &db::now_rfc3339(),
        )?;
        Ok(Json(views::comment_at_own_revision(&row)))
    })
    .await
}

/// The "Range comments" rules of docs/api.md: a range needs a line
/// anchor, ends on it, and is non-empty and forward.
fn validate_range(
    range: types::CommentRange,
    line: Option<i64>,
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
    if range.start_line < 1 || range.start_char < 0 || range.end_char < 1 || !forward {
        return Err(Error::bad_request(
            "range must be non-empty and forward (docs/api.md \"Range comments\")",
        ));
    }
    Ok(range)
}

/// Snapshot of the anchored line: the commit's tree for `new`, the
/// parent commit's tree for `old` (deleted lines live there).
fn anchor_line_text(
    repo: &Repository,
    rev: &db::Revision,
    side: &str,
    file: &str,
    line: i64,
) -> Option<String> {
    let sha = if side == "old" {
        &rev.parent_sha
    } else {
        &rev.commit_sha
    };
    diff::line_text(repo, &diff::commit_tree(repo, sha)?, file, line)
}

async fn edit_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
    AppJson(req): AppJson<types::EditDraft>,
) -> Result<Json<types::Comment>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let comment = load_comment(&conn, id)?;
        if comment.state != "draft" {
            return Err(Error::not_found(format!("draft {id} not found")));
        }
        db::update_draft_body(&conn, id, &req.body, &db::now_rfc3339())?;
        let comment = load_comment(&conn, id)?;
        Ok(Json(views::comment_at_own_revision(&comment)))
    })
    .await
}

async fn delete_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
) -> Result<StatusCode, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let comment = load_comment(&conn, id)?;
        if comment.state != "draft" {
            return Err(Error::not_found(format!("draft {id} not found")));
        }
        db::delete_comment(&conn, id)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

// ---------------------------------------------------------------------------
// Thread resolution (reviewer side)

async fn resolve_comment(
    state: State<Arc<AppState>>,
    path: AppPath<i64>,
) -> Result<Json<types::Comment>, Error> {
    set_resolved(state, path, true).await
}

async fn unresolve_comment(
    state: State<Arc<AppState>>,
    path: AppPath<i64>,
) -> Result<Json<types::Comment>, Error> {
    set_resolved(state, path, false).await
}

async fn set_resolved(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
    resolved: bool,
) -> Result<Json<types::Comment>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let comment = load_comment(&conn, id)?;
        if comment.parent_id.is_some() {
            return Err(Error::bad_request(
                "only root comments carry thread resolution",
            ));
        }
        if comment.state != "published" {
            return Err(Error::bad_request("cannot resolve an unpublished draft"));
        }
        db::comment_set_resolved(&conn, id, resolved, &db::now_rfc3339())?;
        let comment = load_comment(&conn, id)?;
        Ok(Json(views::comment_at_own_revision(&comment)))
    })
    .await
}

// ---------------------------------------------------------------------------
// Reviews

async fn submit_review(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
    AppJson(req): AppJson<types::SubmitReview>,
) -> Result<Json<types::SubmitReviewResponse>, Error> {
    let status = match req.verdict.as_str() {
        "approve" => ChangeStatus::Approved,
        "request_changes" => ChangeStatus::ChangesRequested,
        "comment" => ChangeStatus::Commented,
        other => {
            return Err(Error::bad_request(format!(
                "verdict must be approve | request_changes | comment, got {other:?}"
            )));
        }
    };

    let st = state.clone();
    let chain_id = blocking(move || {
        let conn = st.open_db()?;
        Ok(load_change(&conn, id)?.chain_id)
    })
    .await?;

    // The chain lock: no revision insert (scan) or 409 check happens
    // concurrently with this submission.
    let entry = state.entry(chain_id);
    let gate = entry.gate.lock().await;
    let st = state.clone();
    let result = blocking(move || {
        let mut conn = st.open_db()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let change = load_change(&tx, id)?;
        if change.position.is_none() {
            return Err(Error::conflict(
                "change is orphaned (its commit left the branch) — wait for it to re-attach",
            ));
        }
        let latest = db::latest_revision(&tx, change.id)?
            .ok_or_else(|| Error::internal(format!("change {id} has no revisions")))?;

        let target = if req.revision == latest.number {
            latest.number
        } else {
            let reviewed = db::get_revision(&tx, change.id, req.revision)?.ok_or_else(|| {
                Error::bad_request(format!("revision {} not found", req.revision))
            })?;
            // Pure rebase (patch-id-equal, same message): auto-retarget.
            let retargets = open_repo(&tx, change.chain_id)
                .is_some_and(|repo| gitscan::pure_rebase_equivalent(&repo, &reviewed, &latest));
            if !retargets {
                return Err(Error::conflict(format!(
                    "revision {} is no longer the latest (revision {} landed) — refetch and resubmit",
                    req.revision, latest.number
                )));
            }
            latest.number
        };

        let now = db::now_rfc3339();
        let review = db::insert_review(&tx, change.id, target, &req.verdict, &req.message, &now)?;
        let published = db::publish_drafts(&tx, change.id, review.id, &now)?;
        db::change_set_position_status(&tx, change.id, change.position, status)?;
        db::insert_event(
            &tx,
            change.chain_id,
            "review_submitted",
            &serde_json::json!({
                "chain_id": change.chain_id,
                "change_id": change.id,
                "review_id": review.id,
                "verdict": req.verdict,
            }),
            &now,
        )?;
        db::chain_touch(&tx, change.chain_id, &now)?;
        tx.commit().map_err(anyhow::Error::from)?;
        Ok(Json(types::SubmitReviewResponse {
            review: views::review_json(&review),
            published_comments: published.iter().map(views::comment_at_own_revision).collect(),
        }))
    })
    .await;
    drop(gate);
    if result.is_ok() {
        entry.notify.notify_waiters();
    }
    result
}

// ---------------------------------------------------------------------------
// Agent endpoints

async fn create_reply(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
    AppJson(req): AppJson<types::NewReply>,
) -> Result<Json<types::Comment>, Error> {
    let st = state.clone();
    let (chain_id, reply) = blocking(move || {
        let mut conn = st.open_db()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let comment = load_comment(&tx, id)?;
        if comment.state != "published" {
            return Err(Error::not_found(format!("comment {id} not found")));
        }
        // Thread under the root, wherever the reply pointed.
        let mut root = comment;
        while let Some(parent_id) = root.parent_id {
            root = load_comment(&tx, parent_id)?;
        }
        let change = load_change(&tx, root.change_id)?;
        let now = db::now_rfc3339();
        let reply = db::insert_comment(
            &tx,
            &db::NewComment {
                change_id: root.change_id,
                revision_number: root.revision_number,
                parent_id: Some(root.id),
                author: "agent",
                file: root.file.as_deref(),
                line: root.line,
                side: &root.side,
                range: root.range,
                line_text: root.line_text.as_deref(),
                body: &req.body,
                state: "published",
                resolved: false,
            },
            &now,
        )?;
        if req.resolve {
            db::comment_set_resolved(&tx, root.id, true, &now)?;
        }
        db::insert_event(
            &tx,
            change.chain_id,
            "comment_replied",
            &serde_json::json!({
                "chain_id": change.chain_id,
                "change_id": change.id,
                "comment_id": reply.id,
            }),
            &now,
        )?;
        db::chain_touch(&tx, change.chain_id, &now)?;
        tx.commit().map_err(anyhow::Error::from)?;
        Ok((
            change.chain_id,
            Json(views::comment_at_own_revision(&reply)),
        ))
    })
    .await?;
    state.entry(chain_id).notify.notify_waiters();
    Ok(reply)
}

async fn get_feedback(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
) -> Result<Json<types::Feedback>, Error> {
    feedback_response(state, id).await
}

async fn feedback_response(
    state: Arc<AppState>,
    chain_id: i64,
) -> Result<Json<types::Feedback>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let chain = load_chain(&conn, chain_id)?;
        let repo = open_repo(&conn, chain_id);
        Ok(Json(views::build_feedback(
            &conn,
            repo.as_ref(),
            &state.public_base,
            &chain,
        )?))
    })
    .await
}

#[derive(Deserialize)]
struct WaitQuery {
    cursor: Option<i64>,
    timeout: Option<u64>,
}

/// Long-poll: block until an event with id > cursor exists for this chain
/// (or timeout — default 55s, max 120), then return the latest cursor and
/// a feedback snapshot. `cursor=0` returns the current snapshot
/// immediately.
async fn wait_chain(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<i64>,
    AppQuery(q): AppQuery<WaitQuery>,
) -> Result<Json<types::WaitResponse>, Error> {
    let cursor = q.cursor.unwrap_or(0);
    let timeout = q.timeout.unwrap_or(55).min(120);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout);
    let entry = state.entry(id);
    let mut shutdown = state.shutdown_watch();

    loop {
        // Arm the wakeup *before* checking the db so an event landing
        // between check and sleep cannot be missed.
        let notified = entry.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        let st = state.clone();
        let latest = blocking(move || {
            let conn = st.open_db()?;
            load_chain(&conn, id)?;
            Ok(db::latest_event_id(&conn, id)?)
        })
        .await?;

        if cursor == 0 || latest > cursor {
            break;
        }
        tokio::select! {
            () = &mut notified => {}
            () = tokio::time::sleep_until(deadline) => break,
            // Graceful shutdown: hand back the unchanged snapshot now
            // instead of holding shutdown open for the poll timeout.
            // wait_for checks the current value first, so a poll admitted
            // just after the flip still exits immediately.
            _ = shutdown.wait_for(|&stopping| stopping) => break,
        }
    }

    let st = state.clone();
    let latest = blocking(move || {
        let conn = st.open_db()?;
        Ok(db::latest_event_id(&conn, id)?)
    })
    .await?;
    let Json(feedback) = feedback_response(state, id).await?;
    Ok(Json(types::WaitResponse {
        cursor: latest,
        feedback,
    }))
}
