//! HTTP API: every endpoint of `docs/api.md` (the contract), axum 0.8.
//!
//! - [`types`] — the wire-shape mirror of docs/api.md (golden rule 4).
//! - [`diff`] — diff JSON rendering and line-text snapshots.
//! - [`views`] — the fold (`crate::review`) + drafts → wire shapes.
//! - [`state`] — the in-memory fold, per-chain locks, append/scan, errors.
//!
//! All rusqlite/git2 work runs in `spawn_blocking`; every appender to one
//! chain serializes through its gate and folds in lock-step
//! (docs/data-model.md "Concurrency").

pub mod diff;
pub mod rebase;
pub mod state;
pub mod types;
pub mod views;

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, patch, post};
use futures_util::StreamExt;
use git2::{Oid, Repository};
use serde::Deserialize;

use crate::db;
use crate::enums::{ChainStatus, LogKind, Side};
use crate::gitscan;
use crate::review::{self, CommentInput, Entry, Projection};

pub use state::{AppJson, AppPath, AppQuery, AppState, Error, blocking, scan_chain};

/// The `/api` router. Static UI serving is layered on top in [`app`].
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/repos", get(list_repos))
        .route("/api/repos/{id}", patch(relocate_repo))
        .route("/api/chains", post(register_chain).get(list_chains))
        .route("/api/chains/{id}", get(get_chain))
        .route("/api/chains/{id}/feedback", get(get_feedback))
        .route("/api/chains/{id}/events", get(events_chain))
        .route("/api/chains/{id}/log", get(log_chain))
        .route("/api/changes/{id}", get(get_change_detail))
        .route("/api/changes/{id}/revisions/{n}/diff", get(revision_diff))
        .route("/api/changes/{id}/drafts", post(create_draft))
        .route("/api/changes/{id}/comments", post(create_comment))
        .route("/api/changes/{id}/reviews", post(submit_review))
        .route("/api/drafts/{id}", patch(edit_draft).delete(delete_draft))
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

/// Serve `app` on an already-bound listener until `shutdown` resolves.
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
    let state = AppState::load(db_path, format!("http://{addr}"))?;
    tracing::info!("listening on http://{addr}");
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
// Routing helpers (id → chain), all reading the in-memory fold

/// The chain entry owning `change_id`, plus its chain id.
fn entry_of_change(
    state: &Arc<AppState>,
    change_id: u64,
) -> Result<(Arc<state::ChainEntry>, u64), Error> {
    for id in state.chain_ids() {
        if let Some(entry) = state.chain_entry(id)
            && entry.read().change_by_id(change_id).is_some()
        {
            return Ok((entry, id));
        }
    }
    Err(Error::not_found(format!("change {change_id} not found")))
}

fn entry_or_404(state: &Arc<AppState>, chain_id: u64) -> Result<Arc<state::ChainEntry>, Error> {
    state
        .chain_entry(chain_id)
        .ok_or_else(|| Error::not_found(format!("chain {chain_id} not found")))
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
// Repos (the registry grouping for chains)

/// Active (non-merged/abandoned) chain count per repo id, read from the fold.
fn active_chains_by_repo(state: &Arc<AppState>) -> HashMap<u64, u64> {
    let mut active: HashMap<u64, u64> = HashMap::new();
    for id in state.chain_ids() {
        if let Some(entry) = state.chain_entry(id) {
            let proj = entry.read();
            if proj.status == ChainStatus::Active {
                *active.entry(proj.repo_id).or_default() += 1;
            }
        }
    }
    active
}

/// List every registered repo with its active-chain count (the web main
/// page). Rescans (throttled) first, like the dashboard, so the counts are
/// current; the count is derived from the fold, never stored.
async fn list_repos(State(state): State<Arc<AppState>>) -> Result<Json<types::RepoList>, Error> {
    let ids = state.chain_ids();
    for id in &ids {
        scan_chain(&state, *id, false).await?;
    }
    blocking(move || {
        let conn = state.open_db()?;
        let active = active_chains_by_repo(&state);
        let repos = db::all_repos(&conn)?
            .into_iter()
            .map(|r| types::Repo {
                id: r.id,
                git_dir: r.git_dir,
                active_chains: active.get(&r.id).copied().unwrap_or(0),
            })
            .collect();
        Ok(Json(types::RepoList { repos }))
    })
    .await
}

/// Repoint a repo at a new git-common-dir after it moved on disk
/// (`nit repo move`). Validates and canonicalizes the new path, updates the
/// registry, then refreshes every loaded chain of that repo so subsequent
/// scans open the new path (docs/api.md "Repos").
async fn relocate_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
    AppJson(req): AppJson<types::RelocateRepo>,
) -> Result<Json<types::Repo>, Error> {
    let st = state.clone();
    let new_git_dir = blocking(move || -> Result<String, Error> {
        let conn = st.open_db()?;
        if db::get_repo(&conn, repo_id)?.is_none() {
            return Err(Error::not_found(format!("repo {repo_id} not found")));
        }
        let canonical = std::fs::canonicalize(&req.git_dir).map_err(|e| {
            Error::bad_request(format!("cannot resolve git dir {}: {e}", req.git_dir))
        })?;
        let canonical = canonical
            .to_str()
            .ok_or_else(|| Error::bad_request("git dir is not valid UTF-8"))?
            .to_string();
        Repository::open(&canonical).map_err(|e| {
            Error::bad_request(format!(
                "not a git repository at {canonical}: {}",
                e.message()
            ))
        })?;
        if let Some(other) = db::find_repo(&conn, &canonical)?
            && other.id != repo_id
        {
            return Err(Error::conflict(format!(
                "git dir {canonical} is already registered as repo {}",
                other.id
            )));
        }
        db::update_repo_git_dir(&conn, repo_id, &canonical)?;
        Ok(canonical)
    })
    .await?;
    // Refresh the in-memory chains so scans open the new path (the projection
    // caches the git dir; ensure_entry only refreshes base).
    for id in state.chain_ids() {
        if let Some(entry) = state.chain_entry(id) {
            let mut proj = entry.proj.write().expect("projection lock poisoned");
            if proj.repo_id == repo_id {
                proj.git_dir.clone_from(&new_git_dir);
            }
        }
    }
    Ok(Json(types::Repo {
        id: repo_id,
        git_dir: new_git_dir,
        active_chains: active_chains_by_repo(&state)
            .get(&repo_id)
            .copied()
            .unwrap_or(0),
    }))
}

// ---------------------------------------------------------------------------
// Chains

async fn register_chain(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<types::RegisterChain>,
) -> Result<Json<types::Chain>, Error> {
    let partial = req.partial;
    let st = state.clone();
    let chain_id = blocking(move || {
        let conn = st.open_db()?;
        let canonical = std::fs::canonicalize(&req.git_dir).map_err(|e| {
            Error::bad_request(format!("cannot resolve git dir {}: {e}", req.git_dir))
        })?;
        let canonical = canonical
            .to_str()
            .ok_or_else(|| Error::bad_request("git dir is not valid UTF-8"))?;
        // A *new* chain validates (the 400 case); an existing one re-registers
        // even mid-rebase, surfacing failures as last_scan_error. Validate
        // before touching the registry so a bad branch/base never leaves an
        // orphan repo row behind.
        let is_new_chain = match db::find_repo(&conn, canonical)? {
            Some(repo) => db::find_chain(&conn, repo.id, &req.branch)?.is_none(),
            None => true,
        };
        if is_new_chain {
            gitscan::validate_registration(FsPath::new(canonical), &req.branch, &req.base)
                .map_err(|e| Error::bad_request(format!("{e:#}")))?;
        }
        let repo = db::get_or_create_repo(&conn, canonical)?;
        let chain = db::get_or_create_chain(&conn, repo.id, &req.branch, &req.base)?;
        st.ensure_entry(&conn, &chain)?;
        Ok(chain.id)
    })
    .await?;
    // Scan before applying partial (state.rs scan_then_flip rationale): a
    // `nit ready` carrying unscanned commits must not let a waiter read the
    // old all-approved set as approved.
    scan_chain(&state, chain_id, true).await?;
    if let Some(partial) = partial {
        apply_partial(&state, chain_id, partial).await?;
    }
    chain_response(state, chain_id).await
}

/// Apply the sticky `partial` flag: append a `partial` entry only on an
/// actual flip. Runs under the chain lock.
async fn apply_partial(state: &Arc<AppState>, chain_id: u64, partial: bool) -> Result<(), Error> {
    let entry = entry_or_404(state, chain_id)?;
    let guard = entry.gate.lock().await;
    let st = state.clone();
    let e2 = entry.clone();
    blocking(move || -> Result<(), Error> {
        if e2.read().partial == partial {
            return Ok(()); // no flip, no entry
        }
        let mut conn = st.open_db()?;
        let payload = serde_json::to_value(review::PartialPayload { partial })
            .map_err(anyhow::Error::from)?;
        let news = vec![(LogKind::Partial, payload)];
        state::commit_entries(&mut conn, &e2, chain_id, news).map_err(map_busy)?;
        Ok(())
    })
    .await?;
    drop(guard);
    Ok(())
}

fn map_busy(err: anyhow::Error) -> Error {
    if state::is_sqlite_busy(&err) {
        Error::unavailable("database is busy (another chain is being scanned) — retry shortly")
    } else {
        err.into()
    }
}

/// Build the Chain JSON for a chain from its current fold.
async fn chain_response(state: Arc<AppState>, chain_id: u64) -> Result<Json<types::Chain>, Error> {
    let entry = entry_or_404(&state, chain_id)?;
    blocking(move || {
        let conn = state.open_db()?;
        let proj = entry.read();
        Ok(Json(views::build_chain(&conn, &state.public_base, &proj)?))
    })
    .await
}

#[derive(Deserialize)]
struct ListChainsQuery {
    status: Option<String>,
    /// Restrict to one repo (the repo-scoped chain view).
    repo: Option<u64>,
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

    let ids = state.chain_ids();
    for id in &ids {
        scan_chain(&state, *id, false).await?;
    }

    blocking(move || {
        let conn = state.open_db()?;
        let mut chains = Vec::new();
        for id in ids {
            let Some(entry) = state.chain_entry(id) else {
                continue;
            };
            let proj = entry.read();
            if !include_closed && proj.status != ChainStatus::Active {
                continue;
            }
            if q.repo.is_some_and(|rid| proj.repo_id != rid) {
                continue;
            }
            chains.push(views::build_chain(&conn, &state.public_base, &proj)?);
        }
        Ok(Json(types::ChainList { chains }))
    })
    .await
}

async fn get_chain(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::Chain>, Error> {
    entry_or_404(&state, id)?;
    scan_chain(&state, id, false).await?;
    chain_response(state, id).await
}

// ---------------------------------------------------------------------------
// Changes

async fn get_change_detail(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::ChangeDetail>, Error> {
    let (entry, chain_id) = entry_of_change(&state, id)?;
    blocking(move || {
        let conn = state.open_db()?;
        let proj = entry.read();
        let change = proj
            .change_by_id(id)
            .ok_or_else(|| Error::not_found(format!("change {id} not found")))?;
        Ok(Json(views::build_change_detail(&conn, chain_id, change)?))
    })
    .await
}

#[derive(Deserialize)]
struct DiffQuery {
    against: Option<u64>,
}

/// Revision `m` of an interdiff (the `against` side), read from the fold:
/// its commit, message, and parent — the parent so a rebase
/// (`parent(m) != parent(n)`) can be detected and contained.
struct AgainstRev {
    commit_sha: String,
    message: String,
    parent_sha: String,
}

async fn revision_diff(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(u64, u64)>,
    AppQuery(q): AppQuery<DiffQuery>,
) -> Result<Json<types::Diff>, Error> {
    let (entry, _) = entry_of_change(&state, id)?;
    blocking(move || {
        // Pull the revision shas + messages out of the fold, then drop the
        // lock before touching git.
        // For an interdiff, `against` also carries parent(m) so a rebase
        // (parent(m) != parent(n)) can be detected and contained.
        let (git_dir, new_sha, new_msg, parent_sha, against): (
            String,
            String,
            String,
            String,
            Option<AgainstRev>,
        ) = {
            let proj = entry.read();
            let change = proj
                .change_by_id(id)
                .ok_or_else(|| Error::not_found(format!("change {id} not found")))?;
            let rev = change
                .revision(n)
                .ok_or_else(|| Error::not_found(format!("revision {n} not found")))?;
            let against = match q.against {
                None => None,
                Some(m) => {
                    let a = change
                        .revision(m)
                        .ok_or_else(|| Error::not_found(format!("revision {m} not found")))?;
                    Some(AgainstRev {
                        commit_sha: a.commit_sha.clone(),
                        message: a.message.clone(),
                        parent_sha: a.parent_sha.clone(),
                    })
                }
            };
            (
                proj.git_dir.clone(),
                rev.commit_sha.clone(),
                rev.message.clone(),
                rev.parent_sha.clone(),
                against,
            )
        };
        let repo = Repository::open(&git_dir)
            .map_err(|e| Error::internal(format!("cannot open the chain's repository: {e}")))?;
        let new_tree = commit_tree(&repo, &new_sha)?;
        // The interdiff's old side (= revision m) and its parent, kept for
        // drift detection after the plain diff is rendered.
        let (old_tree, against_message, against_rev) = match against {
            None => {
                let parent = repo
                    .find_commit(parse_oid(&parent_sha)?)
                    .map_err(|e| Error::internal(format!("parent commit missing: {e}")))?;
                let tree = parent
                    .tree()
                    .map_err(|e| Error::internal(format!("parent tree missing: {e}")))?;
                (tree, None, None)
            }
            Some(a) => (
                commit_tree(&repo, &a.commit_sha)?,
                Some(a.message),
                Some((a.commit_sha, a.parent_sha)),
            ),
        };
        let mut wire = diff::diff_trees(&repo, &old_tree, &new_tree)?;
        wire.files.insert(
            0,
            diff::commit_msg_file(against_message.as_deref(), &new_msg)?,
        );
        // Contain rebase drift only when the interdiff's two revisions have
        // different parents (docs/api.md "Rebase-aware interdiffs").
        if let Some((m_sha, parent_m)) = against_rev
            && parent_m != parent_sha
            && let Err(e) =
                rebase::tag_drift(&repo, &mut wire, &m_sha, &parent_m, &new_sha, &parent_sha)
        {
            tracing::warn!("rebase-aware interdiff tagging failed; serving plain interdiff: {e:#}");
        }
        Ok(Json(wire))
    })
    .await
}

fn commit_tree<'r>(repo: &'r Repository, sha: &str) -> Result<git2::Tree<'r>, Error> {
    diff::commit_tree(repo, sha).ok_or_else(|| Error::internal(format!("tree for {sha} missing")))
}

fn parse_oid(sha: &str) -> Result<Oid, Error> {
    Oid::from_str(sha).map_err(|e| Error::internal(format!("bad sha {sha:?}: {e}")))
}

// ---------------------------------------------------------------------------
// Drafts (reviewer side)

async fn create_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::NewDraft>,
) -> Result<Json<types::Draft>, Error> {
    let (entry, chain_id) = entry_of_change(&state, id)?;
    blocking(move || {
        let conn = state.open_db()?;
        let proj = entry.read();
        let change = proj
            .change_by_id(id)
            .ok_or_else(|| Error::not_found(format!("change {id} not found")))?;
        let rev = change
            .revision(req.revision)
            .ok_or_else(|| Error::bad_request(format!("revision {} not found", req.revision)))?;
        let (side, range) = validate_anchor(req.side, req.file.as_deref(), req.line, req.range)?;
        // An empty body is allowed only for a reply that stages a resolution
        // (docs/api.md "Thread resolution"); a new thread always needs a body,
        // and a resolution with no thread_id resolves nothing.
        let resolution_only = req.thread_id.is_some() && req.resolved.is_some();
        if req.body.trim().is_empty() && !resolution_only {
            return Err(Error::bad_request(
                "a draft needs a body, or a thread_id with a resolution decision",
            ));
        }
        // A reply draft references a thread on this change.
        let thread_id = match req.thread_id {
            Some(tid) => {
                if change.thread(tid).is_none() {
                    return Err(Error::bad_request("thread not found on this change"));
                }
                Some(tid)
            }
            None => None,
        };
        let line_text = snapshot_line_text(&proj.git_dir, rev, req.file.as_deref(), req.line, side);
        let change_key = change.change_key.clone();
        drop(proj);
        let draft_id = state.alloc_id();
        let row = db::insert_draft(
            &conn,
            draft_id,
            &db::NewDraft {
                chain_id,
                change_key: &change_key,
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
    // Lines are 1-based and `end_char` is exclusive, so both must be ≥ 1;
    // `start_char` is 0-based and unsigned, so its lower bound is implicit.
    if range.start_line < 1 || range.end_char < 1 || !forward {
        return Err(Error::bad_request(
            "range must be non-empty and forward (docs/api.md \"Range comments\")",
        ));
    }
    Ok(range)
}

/// Validate a new thread's anchor, shared by reviewer drafts and agent
/// comments: default the `side` to `new`, require a `file` behind any `line`,
/// reject a `/COMMIT_MSG` old-side anchor, and apply the "Range comments"
/// rules. The `side` is a [`Side`], so only these semantic rules need
/// checking — its value is constrained by the type.
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

/// Snapshot the anchored line's text for a new thread: the message line for
/// `/COMMIT_MSG`, else the file's line in the revision's commit (`new` side) or
/// parent (`old` side) tree. `None` for a file-/change-level anchor, or when
/// the repo or line can't be read (the repo may be mid-rebase at draft time —
/// best-effort).
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

async fn edit_draft(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::EditDraft>,
) -> Result<Json<types::Draft>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        db::update_draft(&conn, id, &req.body, req.resolved, &db::now_rfc3339())?;
        let updated = db::get_draft(&conn, id)?
            .ok_or_else(|| Error::not_found(format!("draft {id} not found")))?;
        let change_id = change_id_for_draft(&state, &updated);
        Ok(Json(views::draft_view(&updated, change_id)))
    })
    .await
}

async fn delete_draft(
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

/// The fold-id of the change a draft belongs to (for the wire `change_id`).
fn change_id_for_draft(state: &Arc<AppState>, draft: &db::DraftRow) -> u64 {
    state
        .chain_entry(draft.chain_id)
        .and_then(|e| e.read().change_by_key(&draft.change_key).map(|c| c.id))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Reviews

#[expect(
    clippy::too_many_lines,
    reason = "one atomic flow: resolve target, drain drafts, probe-fold, append, fold, respond"
)]
async fn submit_review(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::SubmitReview>,
) -> Result<Json<types::SubmitReviewResponse>, Error> {
    let (entry, chain_id) = entry_of_change(&state, id)?;
    let guard = entry.gate.lock().await;
    let st = state.clone();
    let e2 = entry.clone();
    let resp = blocking(move || -> Result<Json<types::SubmitReviewResponse>, Error> {
        let mut conn = st.open_db()?;
            // Resolve the target revision + drain drafts under the read lock.
            let (change_key, target, comments, review_id) = {
                let proj = e2.read();
                let change = proj
                    .change_by_id(id)
                    .ok_or_else(|| Error::not_found(format!("change {id} not found")))?;
                if change.orphaned {
                    return Err(Error::conflict(
                        "change is orphaned (its commit left the branch) — wait for it to re-attach",
                    ));
                }
                let latest = change
                    .latest_revision()
                    .ok_or_else(|| Error::internal(format!("change {id} has no revisions")))?;
                let target = resolve_target(&proj, change, latest, req.revision)?;
                let review_id = st.alloc_id();
                let drafts = db::drafts_for_change(&conn, chain_id, &change.change_key)?;
                let comments: Vec<CommentInput> = drafts
                    .iter()
                    .map(|d| CommentInput {
                        thread_id: d.thread_id,
                        revision: Some(d.revision),
                        file: d.file.clone(),
                        line: d.line,
                        // A reply's side is the thread's, not the draft's.
                        side: d.thread_id.is_none().then_some(d.side),
                        range: d.range,
                        line_text: d.line_text.clone(),
                        body: d.body.clone(),
                        resolved: d.resolved,
                    })
                    .collect();
                (change.change_key.clone(), target, comments, review_id)
            };
            // The threads this review continues (replies) — new threads it
            // opens are picked up by the minted-id range below.
            let reply_thread_ids: Vec<u64> = comments.iter().filter_map(|c| c.thread_id).collect();

            let payload = serde_json::to_value(review::ReviewPayload {
                change_key: change_key.clone(),
                review_id,
                revision: target,
                verdict: req.verdict,
                message: req.message.clone(),
                comments,
            })
            .map_err(anyhow::Error::from)?;
            // Drain drafts + append the review entry atomically. Validate the
            // fold on a probe copy first, so a bad payload aborts before any
            // write — the log never gets ahead of the projection
            // (state::commit_entries rationale).
            let now = db::now_rfc3339();
            let start = e2.read().head;
            let parsed = Entry {
                idx: start,
                kind: LogKind::Review,
                payload,
                created_at: now,
            };
            {
                let mut probe = e2.read().clone();
                review::fold(&mut probe, &parsed)?;
            }
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(anyhow::Error::from)?;
            db::delete_drafts_for_change(&tx, chain_id, &change_key).map_err(map_busy)?;
            db::append_log(
                &tx,
                chain_id,
                start,
                parsed.kind.as_str(),
                &parsed.payload,
                &parsed.created_at,
            )
            .map_err(map_busy)?;
            tx.commit().map_err(anyhow::Error::from)?;

            // The threads this review opens get ids from here up (the fold
            // mints them as it applies the new-thread drafts).
            let first_new_thread = e2.read().next_thread_id;
            {
                let mut proj = e2.proj.write().expect("projection lock poisoned");
                review::fold(&mut proj, &parsed).expect("fold validated before commit");
            }
            // Publish the appended review on the live `/events` feed (after
            // the durable commit + fold, as in state::commit_entries).
            e2.publish(views::log_entry_view(&parsed));
            // Build the response from the folded state.
            let proj = e2.read();
            let change = proj
                .change_by_id(id)
                .ok_or_else(|| Error::internal("change vanished after review"))?;
            let review = change
                .reviews
                .iter()
                .find(|r| r.id == review_id)
                .ok_or_else(|| Error::internal("review vanished after fold"))?;
            // The threads this review touched: new-thread drafts minted ids in
            // `first_new_thread..` (monotonic, under the gate), replies
            // continued an existing thread.
            let threads: Vec<types::Thread> = change
                .threads
                .iter()
                .filter(|t| t.id >= first_new_thread || reply_thread_ids.contains(&t.id))
                .map(|t| views::thread_view(t, id))
                .collect();
            Ok(Json(types::SubmitReviewResponse {
                review: views::review_json(review),
                threads,
            }))
        })
        .await?;
    drop(guard);
    Ok(resp)
}

/// The revision a review applies to: the requested one when latest, else the
/// latest if the requested is a pure-rebase ancestor (auto-retarget), else a
/// 409.
fn resolve_target(
    proj: &Projection,
    change: &review::ChangeProj,
    latest: &review::RevisionProj,
    requested: u64,
) -> Result<u64, Error> {
    if requested == latest.number {
        return Ok(latest.number);
    }
    let reviewed = change
        .revision(requested)
        .ok_or_else(|| Error::bad_request(format!("revision {requested} not found")))?;
    let retargets = Repository::open(&proj.git_dir).is_ok_and(|repo| {
        gitscan::pure_rebase(
            &repo,
            &reviewed.commit_sha,
            &reviewed.message,
            &latest.commit_sha,
            &latest.message,
        )
    });
    if retargets {
        Ok(latest.number)
    } else {
        Err(Error::conflict(format!(
            "revision {requested} is no longer the latest (revision {} landed) — refetch and resubmit",
            latest.number
        )))
    }
}

// ---------------------------------------------------------------------------
// Agent endpoints

/// `POST /api/changes/{id}/comments` — the agent's single comment-posting path
/// (docs/api.md "Agent endpoints"). With no `thread_id` it opens a new thread
/// on the change (anchor validated like a reviewer draft; revision defaults to
/// the change's latest, or pins to any earlier one given); with one it appends
/// a reply to that thread. Appends one `comment` log entry and returns the
/// resulting thread (author=agent).
async fn create_comment(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::NewComment>,
) -> Result<Json<types::Thread>, Error> {
    let (entry, chain_id) = entry_of_change(&state, id)?;
    let guard = entry.gate.lock().await;
    let st = state.clone();
    let e2 = entry.clone();
    let resp = blocking(move || -> Result<Json<types::Thread>, Error> {
        // A body is required, except a reply that only changes resolution.
        let resolution_only = req.thread_id.is_some() && req.resolved.is_some();
        if req.body.trim().is_empty() && !resolution_only {
            return Err(Error::bad_request("an agent comment needs a body"));
        }
        // Build the comment under the read lock, then drop it before appending.
        let (change_key, comment) = {
            let proj = e2.read();
            let change = proj
                .change_by_id(id)
                .ok_or_else(|| Error::not_found(format!("change {id} not found")))?;
            let comment = if let Some(tid) = req.thread_id {
                // Reply: the thread must live on this change; its anchor is the
                // thread's, so the payload carries none.
                if change.thread(tid).is_none() {
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
                // New thread: validate the anchor and default the revision to
                // the change's latest (the just-pushed one).
                let (side, range) =
                    validate_anchor(req.side, req.file.as_deref(), req.line, req.range)?;
                let revision = match req.revision {
                    Some(r) => r,
                    None => {
                        change
                            .latest_revision()
                            .ok_or_else(|| {
                                Error::bad_request(format!("change {id} has no revisions"))
                            })?
                            .number
                    }
                };
                let rev = change
                    .revision(revision)
                    .ok_or_else(|| Error::bad_request(format!("revision {revision} not found")))?;
                let line_text =
                    snapshot_line_text(&proj.git_dir, rev, req.file.as_deref(), req.line, side);
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
            };
            (change.change_key.clone(), comment)
        };
        let target_thread = comment.thread_id;
        let payload = serde_json::to_value(review::CommentPayload {
            change_key,
            comment,
        })
        .map_err(anyhow::Error::from)?;
        // A new thread takes the next id about to be minted.
        let first_new_thread = e2.read().next_thread_id;
        let mut conn = st.open_db()?;
        state::commit_entries(&mut conn, &e2, chain_id, vec![(LogKind::Comment, payload)])
            .map_err(map_busy)?;
        let thread_id = target_thread.unwrap_or(first_new_thread);
        let proj = e2.read();
        let thread = proj
            .change_by_id(id)
            .and_then(|c| c.thread(thread_id))
            .ok_or_else(|| Error::internal("thread vanished after fold"))?;
        Ok(Json(views::thread_view(thread, id)))
    })
    .await?;
    drop(guard);
    Ok(resp)
}

async fn get_feedback(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::Feedback>, Error> {
    let entry = entry_or_404(&state, id)?;
    feedback_for(&state, &entry).await
}

async fn feedback_for(
    state: &Arc<AppState>,
    entry: &Arc<state::ChainEntry>,
) -> Result<Json<types::Feedback>, Error> {
    let st = state.clone();
    let e2 = entry.clone();
    blocking(move || {
        let proj = e2.read();
        Ok(Json(views::build_feedback(&st.public_base, &proj)))
    })
    .await
}

#[derive(Deserialize)]
struct EventsQuery {
    cursor: Option<u64>,
}

/// Server-Sent Events stream of a chain's log from `cursor` onward
/// (docs/api.md "events"). On connect it replays every entry already past
/// `cursor` as individual `data: LogEntry` events, then streams each new
/// entry as it is appended; keep-alive comments hold the connection open
/// while the chain is quiet. The server makes **no** wake/relevance
/// judgement — it emits the raw log and leaves "which events matter" to the
/// client (docs/data-model.md "Wake rule"). The stream ends on graceful
/// shutdown or client disconnect.
async fn events_chain(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppQuery(q): AppQuery<EventsQuery>,
) -> Result<impl IntoResponse, Error> {
    let start = q.cursor.unwrap_or(0);
    let chain = entry_or_404(&state, id)?;
    // Subscribe *before* reading the backlog: every entry appended past this
    // point lands on the channel, every earlier one is already in the log, so
    // the two together miss nothing. The overlap — an entry that is both in
    // the backlog and still buffered on the channel — is filtered out below by
    // the `idx` watermark, so each entry surfaces exactly once.
    let live = chain.subscribe();

    let st = state.clone();
    let backlog = blocking(move || {
        let conn = st.open_db()?;
        load_log(&conn, id, start, None)
    })
    .await?;
    let watermark = backlog.last().map_or(start, |e| e.idx + 1);

    let backlog = futures_util::stream::iter(
        backlog
            .into_iter()
            .map(|e| Ok::<Event, Infallible>(sse_event(&e))),
    );
    // Drive `recv()` directly (not the `Stream` impl, which swallows overflow):
    // an `Err` means the channel closed or this subscriber lagged past the
    // buffer, so we end the stream and let the client reconnect + re-read the
    // gap from the log.
    let live = futures_util::stream::unfold(live, |mut rx| async move {
        rx.recv().await.ok().map(|entry| (entry, rx))
    })
    .filter(move |entry| std::future::ready(entry.idx >= watermark))
    .map(|entry| Ok::<Event, Infallible>(sse_event(&entry)));

    let mut shutdown = state.shutdown_watch();
    let stream = backlog.chain(live).take_until(async move {
        let _ = shutdown.wait_for(|&stopping| stopping).await;
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}

/// Render a log entry as an SSE `data:` event, degrading to a comment frame
/// if it somehow fails to serialize (rather than tearing down the stream).
fn sse_event(entry: &types::LogEntry) -> Event {
    Event::default()
        .json_data(entry)
        .unwrap_or_else(|_| Event::default().comment("unserializable entry"))
}

#[derive(Deserialize)]
struct LogQuery {
    from: Option<u64>,
    to: Option<u64>,
}

/// Read-only log slice `[from, to)` (docs/api.md "log"). `from` defaults to
/// `0`, `to` to the head. References past the dataset are an error, not a
/// silent clamp: a closed `to` beyond `head`, or an open `from` beyond
/// `head`, is a 400. A valid range that happens to select nothing (an open
/// `from == head`) returns an empty list.
async fn log_chain(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppQuery(q): AppQuery<LogQuery>,
) -> Result<Json<types::LogResponse>, Error> {
    let entry = entry_or_404(&state, id)?;
    blocking(move || {
        let conn = state.open_db()?;
        let head = entry.read().head;
        let from = q.from.unwrap_or(0);
        let to = match q.to {
            Some(to) if to <= from => {
                return Err(Error::bad_request(format!(
                    "empty or reversed range [{from}, {to}): the end must exceed the start"
                )));
            }
            Some(to) if to > head => {
                return Err(Error::bad_request(format!(
                    "requested entries up to {to} but the log has {head} (valid indices 0..{head})"
                )));
            }
            Some(to) => to,
            None if from > head => {
                return Err(Error::bad_request(format!(
                    "index {from} is past the log head {head} (valid indices 0..{head})"
                )));
            }
            None => head,
        };
        let entries = load_log(&conn, id, from, Some(to))?;
        Ok(Json(types::LogResponse { head, entries }))
    })
    .await
}

/// Load + render log entries `[from, to)`.
fn load_log(
    conn: &rusqlite::Connection,
    chain_id: u64,
    from: u64,
    to: Option<u64>,
) -> Result<Vec<types::LogEntry>, Error> {
    if to.is_some_and(|to| to <= from) {
        return Ok(Vec::new());
    }
    let rows = db::log_entries(conn, chain_id, from, to)?;
    rows.iter()
        .map(|row| Ok(views::log_entry_view(&Entry::from_row(row)?)))
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(Into::into)
}
