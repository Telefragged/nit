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
//! ([`run_lifecycle_timer`]); there are no read-time scans.

pub mod diff;
pub mod rebase;
pub mod state;
pub mod types;
pub mod views;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_broadcast::Receiver;
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post, put};
use git2::{BranchType, Oid, Repository};
use serde::Deserialize;
use tokio_stream::{StreamExt, StreamMap};

use crate::chain::RepoView;
use crate::db;
use crate::enums::{Decision, LifecycleAction, LogKind, Side, Verdict};
use crate::gitscan;
use crate::review::{self, CommentInput, Lifecycle, RevisionPayload};

use types::StreamMsg;

pub use state::{
    AppJson, AppPath, AppQuery, AppState, ChangeEntry, Error, append_to_change,
    append_to_change_with, blocking,
};

/// The `/api` router. Static UI serving is layered on top in [`app`].
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/repos", get(list_repos))
        .route("/api/repos/{id}", patch(relocate_repo))
        .route("/api/push", post(push))
        .route("/api/chains", get(list_chains))
        .route("/api/chains/{id}", get(get_chain))
        .route("/api/chains/{id}/log", get(chain_log))
        .route("/api/chains/{id}/submit", post(submit_chain))
        .route("/api/changes/{id}", get(get_change_detail))
        .route("/api/changes/{id}/revisions/{n}/diff", get(revision_diff))
        .route("/api/changes/{id}/log", get(change_log))
        .route("/api/changes/{id}/drafts", post(create_draft))
        .route("/api/changes/{id}/comments", post(create_comment))
        .route(
            "/api/changes/{id}/decision",
            put(stage_decision).delete(clear_decision),
        )
        .route("/api/changes/{id}/abandon", post(abandon_change))
        .route("/api/changes/{id}/reopen", post(reopen_change))
        .route("/api/drafts/{id}", patch(edit_draft).delete(delete_draft))
        .route("/api/stream", get(stream))
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
    let timer = tokio::spawn(run_lifecycle_timer(state.clone()));
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

// ---------------------------------------------------------------------------
// Health

async fn health() -> Json<types::Health> {
    Json(types::Health {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ---------------------------------------------------------------------------
// Repos

/// A repo row plus its derived live-tip count, as the wire `Repo`.
fn repo_json(state: &AppState, row: db::RepoRow) -> types::Repo {
    let active = u64::try_from(state.repo_view(row.id).tips().len()).unwrap_or(u64::MAX);
    types::Repo {
        id: row.id,
        git_dir: row.git_dir,
        base_branch: row.base_branch,
        active_chains: active,
    }
}

/// List every registered repo with its live-tip count (derived, never stored).
async fn list_repos(State(state): State<Arc<AppState>>) -> Result<Json<types::RepoList>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let repos = db::all_repos(&conn)?
            .into_iter()
            .map(|r| repo_json(&state, r))
            .collect();
        Ok(Json(types::RepoList { repos }))
    })
    .await
}

/// Canonicalize a git-dir path to a UTF-8 string, or a 400.
fn canonical_git_dir(raw: &str) -> Result<String, Error> {
    Ok(std::fs::canonicalize(raw)
        .map_err(|e| Error::bad_request(format!("cannot resolve git dir {raw}: {e}")))?
        .to_str()
        .ok_or_else(|| Error::bad_request("git dir is not valid UTF-8"))?
        .to_string())
}

/// Repoint a repo at a new git-common-dir after it moved on disk.
async fn relocate_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
    AppJson(req): AppJson<types::RelocateRepo>,
) -> Result<Json<types::Repo>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let existing = db::get_repo(&conn, repo_id)?
            .ok_or_else(|| Error::not_found(format!("repo {repo_id} not found")))?;
        let canonical = canonical_git_dir(&req.git_dir)?;
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
        let row = db::RepoRow {
            id: repo_id,
            git_dir: canonical,
            base_branch: existing.base_branch,
        };
        state.ensure_repo(&row);
        Ok(Json(repo_json(&state, row)))
    })
    .await
}

// ---------------------------------------------------------------------------
// Push

/// One push target: a walked change's entry + id (push pre-flight → append).
struct Target {
    entry: Arc<ChangeEntry>,
    change_id: u64,
}

#[expect(
    clippy::too_many_lines,
    reason = "one push flow: resolve, walk, pre-flight, per-commit upsert+append, partial, result"
)]
async fn push(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<types::PushRequest>,
) -> Result<Json<types::PushResult>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let canonical = canonical_git_dir(&req.git_dir)?;
        let repo = Repository::open(&canonical)
            .map_err(|e| Error::internal(format!("cannot open repository: {e}")))?;

        // The repo's one canonical branch: the stored one for a known repo
        // (an explicit base must match it), else auto-detected on first push.
        let base = resolve_base(&repo, &conn, &canonical, req.base.as_deref())?;

        let walk = gitscan::walk_push(&canonical, &base, &req.tip).map_err(Error::bad_request)?;
        // A tip that is ancestor-or-equal of the base walks to nothing: the work
        // already landed (or you pushed the base itself). Reject it loudly rather
        // than recording nothing, so a stray push of a merged commit is a visible
        // mistake, not a silent no-op (docs/data-model.md "Push").
        if walk.commits.is_empty() {
            return Err(Error::conflict(format!(
                "tip {} is already merged into '{}' — no commits to review",
                gitscan::short_sha(&walk.fork_sha),
                base
            )));
        }
        let repo_row = db::get_or_create_repo(&conn, &canonical, &base)?;
        state.ensure_repo(&repo_row);

        // Pre-flight: ensure every change exists, and reject (409) a push that
        // would add a revision to an abandoned change.
        let mut targets = Vec::with_capacity(walk.commits.len());
        for wc in &walk.commits {
            let change_id = db::upsert_change(&conn, repo_row.id, &wc.change_key)?;
            let row = db::get_change(&conn, change_id)?
                .ok_or_else(|| Error::internal("change vanished after upsert"))?;
            let entry = state.ensure_change(&conn, &row)?;
            let proj = entry.read();
            let moves = proj
                .latest_revision()
                .is_none_or(|r| r.commit_sha != wc.commit_sha);
            if moves && matches!(proj.lifecycle, Lifecycle::Abandoned) {
                return Err(Error::conflict(format!(
                    "change {} is abandoned — run `nit reopen` before pushing a new revision",
                    wc.change_key
                )));
            }
            drop(proj);
            targets.push(Target { entry, change_id });
        }

        // Per commit, oldest-first: append a revision iff the content moved.
        for (i, (wc, t)) in walk.commits.iter().zip(&targets).enumerate() {
            let prior = t.entry.read().latest_revision().cloned();
            if prior
                .as_ref()
                .is_some_and(|r| r.commit_sha == wc.commit_sha)
            {
                continue; // unchanged
            }
            let resets_status = match &prior {
                Some(old) => !gitscan::pure_rebase(
                    &repo,
                    &old.commit_sha,
                    &old.message,
                    &wc.commit_sha,
                    &wc.message,
                ),
                None => true,
            };
            let partial = req
                .partial
                .unwrap_or_else(|| prior.as_ref().is_some_and(|r| r.partial));
            let payload = serde_json::to_value(RevisionPayload {
                commit_sha: wc.commit_sha.clone(),
                parent_sha: wc.parent_sha.clone(),
                base_sha: walk.fork_sha.clone(),
                message: wc.message.clone(),
                partial,
                resets_status,
            })
            .map_err(anyhow::Error::from)?;
            let mut c = state.open_db()?;
            append_to_change(
                &mut c,
                &t.entry,
                t.change_id,
                vec![(LogKind::Revision, payload)],
            )
            .map_err(map_busy)?;
            gitscan::maintain_keep_refs(&repo, &t.entry.read());

            // A newly established parent↔child edge tells followers to
            // re-derive (advisory — they re-derive HEAD regardless). Publish on
            // the edge's *pre-existing* endpoint, the only feed a follower can
            // already hold: a re-rooted existing change on its own feed; a
            // brand-new child stacked on an existing parent, on the parent's.
            if i > 0 {
                let parent = &targets[i - 1];
                let feed = match &prior {
                    Some(old) if old.parent_sha != wc.parent_sha => Some(&t.entry),
                    None => Some(&parent.entry),
                    _ => None,
                };
                if let Some(feed) = feed {
                    feed.publish(StreamMsg::NewParent {
                        new_parent: types::NewParent {
                            of: t.change_id,
                            parent: parent.change_id,
                        },
                    });
                }
            }
        }

        // The tip's partial flag (sticky). Re-stamp it when `req.partial`
        // differs from the tip's latest revision — this is what `nit ready`
        // (no revision moved) flips.
        if let (Some(req_partial), Some(tip)) = (req.partial, targets.last()) {
            let current = tip.entry.read().latest_revision().map(|r| r.partial);
            if current != Some(req_partial) {
                let payload = serde_json::to_value(review::PartialPayload {
                    partial: req_partial,
                })
                .map_err(anyhow::Error::from)?;
                let mut c = state.open_db()?;
                append_to_change(
                    &mut c,
                    &tip.entry,
                    tip.change_id,
                    vec![(LogKind::Partial, payload)],
                )
                .map_err(map_busy)?;
            }
        }

        // Build the result from the derived chain rooted at the tip. The
        // empty-walk guard above guarantees at least one target.
        let view = state.repo_view(repo_row.id);
        let tip = targets
            .last()
            .expect("the empty-walk guard guarantees at least one target");
        let tip_change = {
            let proj = tip.entry.read();
            let rev = proj.latest_revision();
            types::TipChange {
                change_id: tip.change_id,
                change_key: proj.change_key.clone(),
                revision: rev.map_or(0, |r| r.number),
                status: rev.map_or(crate::enums::ChangeStatus::Pending, |r| {
                    proj.status_at(r.number)
                }),
            }
        };
        let tip_sha = walk
            .commits
            .last()
            .map_or(walk.fork_sha.clone(), |c| c.commit_sha.clone());
        let chain = views::build_chain(
            &conn,
            &repo,
            &view,
            repo_row.id,
            &repo_row.base_branch,
            &tip_sha,
        )?;
        Ok(Json(types::PushResult { tip_change, chain }))
    })
    .await
}

fn map_busy(err: anyhow::Error) -> Error {
    if state::is_sqlite_busy(&err) {
        Error::unavailable("database is busy (another change is being written) — retry shortly")
    } else {
        err.into()
    }
}

/// The repo's one canonical base branch for this push. A known repo reuses its
/// stored branch (an explicit `base` must match it, else a 400 — one base per
/// repo). A fresh repo takes an explicit `base` as given, or auto-detects when
/// none is passed.
fn resolve_base(
    repo: &Repository,
    conn: &rusqlite::Connection,
    canonical: &str,
    requested: Option<&str>,
) -> Result<String, Error> {
    match (requested, db::find_repo(conn, canonical)?) {
        (Some(req), Some(row)) if req != row.base_branch => Err(Error::bad_request(format!(
            "repo's canonical branch is '{}', not '{req}' — a repo has one base",
            row.base_branch
        ))),
        (Some(req), _) => Ok(req.to_string()),
        (None, Some(row)) => Ok(row.base_branch),
        (None, None) => detect_base(repo),
    }
}

/// Auto-detect the canonical branch on a repo's first push: the local `main` or
/// `master`, whichever exists. Neither or both is ambiguous — a 400 asking the
/// caller to specify `base` (which client surfaces that is the client's affair).
fn detect_base(repo: &Repository) -> Result<String, Error> {
    let has = |name| repo.find_branch(name, BranchType::Local).is_ok();
    match (has("main"), has("master")) {
        (true, false) => Ok("main".to_string()),
        (false, true) => Ok("master".to_string()),
        (true, true) => Err(Error::bad_request(
            "repo has both 'main' and 'master' — specify the base branch explicitly",
        )),
        (false, false) => Err(Error::bad_request(
            "no 'main' or 'master' branch found — specify the base branch explicitly",
        )),
    }
}

// ---------------------------------------------------------------------------
// Chains (derived, on demand)

/// `?status=` filter: active-only (default) or all (includes terminal chains).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChainFilter {
    #[default]
    Active,
    All,
}

#[derive(Deserialize)]
struct ListChainsQuery {
    #[serde(default)]
    status: ChainFilter,
    repo: Option<u64>,
}

async fn list_chains(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ListChainsQuery>,
) -> Result<Json<types::ChainList>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let include_terminal = matches!(q.status, ChainFilter::All);
        let mut chains = Vec::new();
        for repo_id in state.repo_ids() {
            if q.repo.is_some_and(|r| r != repo_id) {
                continue;
            }
            let Some(repo_state) = state.repo_state(repo_id) else {
                continue;
            };
            let repo = Repository::open(repo_state.git_dir())
                .map_err(|e| Error::internal(format!("cannot open repository: {e}")))?;
            let view = state.repo_view(repo_id);
            let tips = if include_terminal {
                view.all_tips()
            } else {
                view.tips()
            };
            for tip in tips {
                chains.push(views::build_chain_summary(
                    &conn, &repo, &view, repo_id, &tip,
                )?);
            }
        }
        Ok(Json(types::ChainList { chains }))
    })
    .await
}

#[derive(Deserialize)]
struct ChainQuery {
    revision: Option<u64>,
}

async fn get_chain(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<u64>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<types::Chain>, Error> {
    let entry = change_or_404(&state, change_id)?;
    blocking(move || {
        let conn = state.open_db()?;
        let repo = repo_of_change(&state, &entry)?;
        let repo_id = entry.read().repo_id;
        let base_branch = state
            .repo_state(repo_id)
            .ok_or_else(|| Error::internal(format!("repo {repo_id} not loaded")))?
            .base_branch
            .clone();
        let view = state.repo_view(repo_id);
        let revision = q
            .revision
            .or_else(|| {
                view.change(change_id)
                    .and_then(|c| c.latest_revision().map(|r| r.number))
            })
            .ok_or_else(|| Error::not_found(format!("change {change_id} has no revisions")))?;
        let tip_sha = views::tip_for(&view, change_id, revision)
            .ok_or_else(|| Error::not_found(format!("revision {revision} not found")))?;
        Ok(Json(views::build_chain(
            &conn,
            &repo,
            &view,
            repo_id,
            &base_branch,
            &tip_sha,
        )?))
    })
    .await
}

/// The aggregated chain log: every member's entries, sorted by global `seq`.
async fn chain_log(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<u64>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<types::ChainLog>, Error> {
    let entry = change_or_404(&state, change_id)?;
    blocking(move || {
        let conn = state.open_db()?;
        let repo_id = entry.read().repo_id;
        let view = state.repo_view(repo_id);
        let revision = q
            .revision
            .or_else(|| {
                view.change(change_id)
                    .and_then(|c| c.latest_revision().map(|r| r.number))
            })
            .unwrap_or(0);
        let tip_sha = views::tip_for(&view, change_id, revision)
            .ok_or_else(|| Error::not_found(format!("change {change_id} has no revisions")))?;
        let path = view.path_from_tip(&tip_sha);
        let mut entries = Vec::new();
        for member in &path {
            for row in db::log_entries(&conn, member.change_id, 0, None)? {
                entries.push(views::log_entry_view(
                    member.change_id,
                    &review::Entry::from_row(&row)?,
                ));
            }
        }
        entries.sort_by_key(|e| e.seq);
        Ok(Json(types::ChainLog { entries }))
    })
    .await
}

// ---------------------------------------------------------------------------
// Changes

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

async fn get_change_detail(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::ChangeDetail>, Error> {
    let entry = change_or_404(&state, id)?;
    blocking(move || {
        let conn = state.open_db()?;
        change_detail_json(&conn, &state, &entry, id)
    })
    .await
}

#[derive(Deserialize)]
struct DiffQuery {
    against: Option<u64>,
}

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
    let entry = change_or_404(&state, id)?;
    blocking(move || {
        let (git_dir, new_sha, new_msg, parent_sha, against): (
            String,
            String,
            String,
            String,
            Option<AgainstRev>,
        ) = {
            let proj = entry.read();
            let rev = proj
                .revision(n)
                .ok_or_else(|| Error::not_found(format!("revision {n} not found")))?;
            let against = match q.against {
                None => None,
                Some(m) => {
                    let a = proj
                        .revision(m)
                        .ok_or_else(|| Error::not_found(format!("revision {m} not found")))?;
                    Some(AgainstRev {
                        commit_sha: a.commit_sha.clone(),
                        message: a.message.clone(),
                        parent_sha: a.parent_sha.clone(),
                    })
                }
            };
            let repo_id = proj.repo_id;
            let git_dir = state
                .repo_state(repo_id)
                .ok_or_else(|| Error::internal("repo not loaded"))?
                .git_dir();
            (
                git_dir,
                rev.commit_sha.clone(),
                rev.message.clone(),
                rev.parent_sha.clone(),
                against,
            )
        };
        let repo = Repository::open(&git_dir)
            .map_err(|e| Error::internal(format!("cannot open the repository: {e}")))?;
        let new_tree = commit_tree(&repo, &new_sha)?;
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

#[derive(Deserialize)]
struct LogQuery {
    from: Option<u64>,
    to: Option<u64>,
}

/// Read-only single-change log slice `[from, to)` (docs/api.md). References
/// past the dataset are a 400, not a silent clamp.
async fn change_log(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppQuery(q): AppQuery<LogQuery>,
) -> Result<Json<types::LogResponse>, Error> {
    let entry = change_or_404(&state, id)?;
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
                    "requested entries up to {to} but the change has {head} (valid 0..{head})"
                )));
            }
            Some(to) => to,
            None if from > head => {
                return Err(Error::bad_request(format!(
                    "index {from} is past the log head {head} (valid 0..{head})"
                )));
            }
            None => head,
        };
        let entries = db::log_entries(&conn, id, from, Some(to))?
            .iter()
            .map(|row| Ok(views::log_entry_view(id, &review::Entry::from_row(row)?)))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Json(types::LogResponse { head, entries }))
    })
    .await
}

// ---------------------------------------------------------------------------
// Drafts (reviewer side)

async fn create_draft(
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
        Ok(Json(views::draft_view(&updated, updated.change_id)))
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

// ---------------------------------------------------------------------------
// Reviews + reviewer decisions

/// One change's reviewer comment drafts as `CommentInput`s, ready to drain into
/// a `review` entry (a reply keeps its thread, a new thread carries its anchor).
fn drafts_to_comments(
    conn: &rusqlite::Connection,
    change_id: u64,
) -> anyhow::Result<Vec<CommentInput>> {
    Ok(db::drafts_for_change(conn, change_id)?
        .iter()
        .map(|d| CommentInput {
            thread_id: d.thread_id,
            revision: Some(d.revision),
            file: d.file.clone(),
            line: d.line,
            side: d.thread_id.is_none().then_some(d.side),
            range: d.range,
            line_text: d.line_text.clone(),
            body: d.body.clone(),
            resolved: d.resolved,
        })
        .collect())
}

/// Publish one reviewer `decision` for a change in **one** per-change
/// transaction (docs/data-model.md "Reviewer decisions"): a `reopen` lifecycle
/// (so a following review lands on a now-active change), then a `review` entry
/// draining the change's comment drafts (the decision's verdict, or `comment`
/// to carry staged comments when the decision is purely lifecycle), then an
/// `abandon` lifecycle — whichever the decision calls for. The drained comment
/// drafts and the change's `draft_reviews` row are deleted in the same
/// transaction, so a half-published batch never strands work and a re-submit is
/// idempotent. The shared core of `POST /reviews` and the chain batch submit;
/// callers validate the target revision/lifecycle first.
fn publish_member(
    conn: &mut rusqlite::Connection,
    state: &Arc<AppState>,
    entry: &ChangeEntry,
    change_id: u64,
    decision: Decision,
    message: &str,
    revision: u64,
) -> Result<(), Error> {
    let comments = drafts_to_comments(conn, change_id)?;
    let drained = !comments.is_empty();
    // A real verdict publishes as itself; a lifecycle-only decision still
    // publishes a `comment` review when there are comment drafts to carry.
    let verdict = decision
        .as_verdict()
        .or_else(|| drained.then_some(Verdict::Comment));

    let mut news: Vec<(LogKind, serde_json::Value)> = Vec::new();
    if decision.as_lifecycle() == Some(LifecycleAction::Reopened) {
        news.push((
            LogKind::Lifecycle,
            lifecycle_payload(LifecycleAction::Reopened, None)?,
        ));
    }
    if let Some(verdict) = verdict {
        let payload = serde_json::to_value(review::ReviewPayload {
            review_id: state.alloc_id(),
            revision,
            verdict,
            // The cover message rides a real verdict; for a lifecycle decision
            // it is the abandon reason, so the carrier `comment` review has none.
            message: if decision.as_verdict().is_some() {
                message.to_string()
            } else {
                String::new()
            },
            comments,
        })
        .map_err(anyhow::Error::from)?;
        news.push((LogKind::Review, payload));
    }
    if decision.as_lifecycle() == Some(LifecycleAction::Abandoned) {
        let reason = (!message.trim().is_empty()).then(|| message.to_string());
        news.push((
            LogKind::Lifecycle,
            lifecycle_payload(LifecycleAction::Abandoned, reason)?,
        ));
    }

    append_to_change_with(conn, entry, change_id, news, |tx| {
        if drained {
            db::delete_drafts_for_change(tx, change_id)?;
        }
        db::delete_draft_review(tx, change_id)
    })
    .map_err(map_busy)?;
    Ok(())
}

/// A `lifecycle` payload value (revision is set only by the merge timer).
fn lifecycle_payload(
    action: LifecycleAction,
    message: Option<String>,
) -> Result<serde_json::Value, Error> {
    serde_json::to_value(review::LifecyclePayload {
        action,
        revision: None,
        message,
    })
    .map_err(|e| anyhow::Error::from(e).into())
}

/// `PUT /api/changes/{id}/decision` — stage (or overwrite) the change's draft
/// decision. Validated only as an enum; legality against the lifecycle is a
/// submit-time concern (a draft is reviewer scratch).
async fn stage_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::StagedDecision>,
) -> Result<Json<types::StagedDecision>, Error> {
    change_or_404(&state, id)?;
    blocking(move || {
        let conn = state.open_db()?;
        db::upsert_draft_review(&conn, id, req.decision, &req.message)?;
        Ok(Json(req))
    })
    .await
}

/// `DELETE /api/changes/{id}/decision` — discard the staged decision (204; a
/// no-op when nothing is staged).
async fn clear_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<StatusCode, Error> {
    change_or_404(&state, id)?;
    blocking(move || {
        let conn = state.open_db()?;
        db::delete_draft_review(&conn, id)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// `POST /api/chains/{id}/submit` — publish every chain member's staged
/// decision (docs/api.md "Chains"). Re-derives the path, then for each member
/// with a decision publishes it at the revision this path pins on the member,
/// each in its own transaction (atomic per change, not across the chain). A
/// decision illegal for the member's current lifecycle is skipped into
/// `errors` with its row kept; a published decision's row is deleted, so a
/// re-submit finishes a torn batch without double-publishing.
async fn submit_chain(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<u64>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<types::BatchSubmitResult>, Error> {
    let entry = change_or_404(&state, change_id)?;
    blocking(move || {
        let mut conn = state.open_db()?;
        let repo_id = entry.read().repo_id;
        let view = state.repo_view(repo_id);
        let revision = q
            .revision
            .or_else(|| {
                view.change(change_id)
                    .and_then(|c| c.latest_revision().map(|r| r.number))
            })
            .ok_or_else(|| Error::not_found(format!("change {change_id} has no revisions")))?;
        let tip_sha = views::tip_for(&view, change_id, revision)
            .ok_or_else(|| Error::not_found(format!("revision {revision} not found")))?;

        let mut submitted = 0u64;
        let mut errors = Vec::new();
        for member in view.path_from_tip(&tip_sha) {
            let Some(staged) = db::get_draft_review(&conn, member.change_id)? else {
                continue; // no decision on this member — leave its comment drafts
            };
            let Some(member_entry) = state.change_entry(member.change_id) else {
                continue;
            };
            let lifecycle = member_entry.read().lifecycle;
            if let Some(reason) = decision_block(lifecycle, staged.decision) {
                errors.push(types::SubmitError {
                    change_id: member.change_id,
                    message: reason.to_string(),
                });
                continue;
            }
            match publish_member(
                &mut conn,
                &state,
                &member_entry,
                member.change_id,
                staged.decision,
                &staged.message,
                member.revision,
            ) {
                Ok(()) => submitted += 1,
                Err(e) => errors.push(types::SubmitError {
                    change_id: member.change_id,
                    message: e.message,
                }),
            }
        }
        Ok(Json(types::BatchSubmitResult { submitted, errors }))
    })
    .await
}

/// Why a staged decision cannot publish against the member's current lifecycle,
/// or `None` when it is legal (an active change takes any verdict or `abandon`;
/// an abandoned change takes only `reopen`).
fn decision_block(lifecycle: Lifecycle, decision: Decision) -> Option<&'static str> {
    match (lifecycle, decision.as_lifecycle()) {
        (Lifecycle::Merged { .. }, _) => Some("change is merged — nothing to submit"),
        (Lifecycle::Abandoned, Some(LifecycleAction::Reopened)) => None,
        (Lifecycle::Abandoned, _) => Some("change is abandoned — stage Reopen first"),
        (Lifecycle::Active, Some(LifecycleAction::Reopened)) => {
            Some("change is live — Reopen does not apply")
        }
        (Lifecycle::Active, _) => None,
    }
}

// ---------------------------------------------------------------------------
// Agent endpoints

async fn create_comment(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::NewComment>,
) -> Result<Json<types::Thread>, Error> {
    let entry = change_or_404(&state, id)?;
    blocking(move || {
        let mut conn = state.open_db()?;
        let resolution_only = req.thread_id.is_some() && req.resolved.is_some();
        if req.body.trim().is_empty() && !resolution_only {
            return Err(Error::bad_request("an agent comment needs a body"));
        }
        let (comment, first_new_thread) = {
            let proj = entry.read();
            let comment = if let Some(tid) = req.thread_id {
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
                let git_dir = state
                    .repo_state(proj.repo_id)
                    .ok_or_else(|| Error::internal("repo not loaded"))?
                    .git_dir();
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
            };
            (comment, proj.next_thread_id)
        };
        let target_thread = comment.thread_id;
        let payload = serde_json::to_value(review::CommentPayload { comment })
            .map_err(anyhow::Error::from)?;
        append_to_change(&mut conn, &entry, id, vec![(LogKind::Comment, payload)])
            .map_err(map_busy)?;
        let thread_id = target_thread.unwrap_or(first_new_thread);
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
    state: &Arc<AppState>,
    entry: &ChangeEntry,
    id: u64,
    guard: fn(&Lifecycle) -> bool,
    action: LifecycleAction,
    message: Option<String>,
) -> Result<Json<types::ChangeDetail>, Error> {
    let mut conn = state.open_db()?;
    if guard(&entry.read().lifecycle) {
        let payload = serde_json::to_value(review::LifecyclePayload {
            action,
            revision: None,
            message,
        })
        .map_err(anyhow::Error::from)?;
        append_to_change(&mut conn, entry, id, vec![(LogKind::Lifecycle, payload)])
            .map_err(map_busy)?;
    }
    change_detail_json(&conn, state, entry, id)
}

/// `POST /api/changes/{id}/abandon` — mark a live change abandoned
/// (`nit abandon`): a reviewer/agent judgment, never automatic. Optional
/// `message` records a reason. A no-op on an already-terminal change.
async fn abandon_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<types::AbandonRequest>,
) -> Result<Json<types::ChangeDetail>, Error> {
    let entry = change_or_404(&state, id)?;
    blocking(move || {
        set_lifecycle(
            &state,
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
async fn reopen_change(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::ChangeDetail>, Error> {
    let entry = change_or_404(&state, id)?;
    blocking(move || {
        set_lifecycle(
            &state,
            &entry,
            id,
            |l| matches!(l, Lifecycle::Abandoned),
            LifecycleAction::Reopened,
            None,
        )
    })
    .await
}

// ---------------------------------------------------------------------------
// Events (WS /api/stream)

/// `WS /api/stream?repo={id}` — the client-driven change stream
/// (docs/api.md "Events"). The `repo` query is accepted for symmetry and
/// ignored; the server keys purely on the subscribed change ids.
async fn stream(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Drive one follower's socket: `subscribe` messages drive a keyed
/// `StreamMap` of per-change feeds (dynamic membership); each arms the feed
/// **before** replaying the change's `[from, head)` backlog and records an idx
/// watermark so the arm/read overlap is deduped, never gapped.
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut feeds: StreamMap<u64, Receiver<StreamMsg>> = StreamMap::new();
    let mut watermark: HashMap<u64, u64> = HashMap::new();
    let mut shutdown = state.shutdown_watch();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(msg)) = incoming else { break };
                match msg {
                    Message::Text(text) => {
                        let Ok(client) = serde_json::from_str::<types::ClientMsg>(&text) else {
                            continue;
                        };
                        if apply_client_msg(&mut socket, &state, &mut feeds, &mut watermark, client)
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {} // ping/pong/binary: ignored
                }
            }
            Some((change_id, msg)) = feeds.next(), if !feeds.is_empty() => {
                // Drop a live entry the backlog replay already covered.
                if let StreamMsg::Entry(ref e) = msg
                    && e.idx < watermark.get(&change_id).copied().unwrap_or(0)
                {
                    continue;
                }
                if send_json(&mut socket, &msg).await.is_err() {
                    break;
                }
            }
            // The only change to the shutdown signal is false → true.
            _ = shutdown.changed() => break,
        }
    }
}

/// Apply one client message; `Err(())` means the socket should close.
async fn apply_client_msg(
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    feeds: &mut StreamMap<u64, Receiver<StreamMsg>>,
    watermark: &mut HashMap<u64, u64>,
    client: types::ClientMsg,
) -> Result<(), ()> {
    match client {
        types::ClientMsg::Subscribe(map) => {
            for (id_str, from) in map {
                let Ok(change_id) = id_str.parse::<u64>() else {
                    continue;
                };
                let Some(entry) = state.change_entry(change_id) else {
                    continue;
                };
                // Arm the live feed BEFORE reading the backlog.
                feeds.insert(change_id, entry.subscribe());
                let backlog = read_backlog(state, change_id, from).await;
                let mut next = from;
                for e in &backlog {
                    next = e.idx + 1;
                    send_json(socket, &StreamMsg::Entry(e.clone())).await?;
                }
                watermark.insert(change_id, next);
            }
        }
    }
    Ok(())
}

async fn send_json(socket: &mut WebSocket, msg: &StreamMsg) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// A change's log slice `[from, head)` as tagged entries, for the backlog
/// replay. Errors collapse to empty (the follower re-reads on reconnect).
async fn read_backlog(state: &Arc<AppState>, change_id: u64, from: u64) -> Vec<types::LogEntry> {
    let st = state.clone();
    blocking(move || {
        let conn = st.open_db()?;
        let rows = db::log_entries(&conn, change_id, from, None)?;
        rows.iter()
            .map(|r| {
                Ok(views::log_entry_view(
                    change_id,
                    &review::Entry::from_row(r)?,
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(Into::into)
    })
    .await
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Lifecycle timer (merged only; abandonment is the explicit abandon action)

/// Interval between timer sweeps, env-configurable for tests.
fn timer_interval() -> Duration {
    Duration::from_millis(
        std::env::var("NIT_TIMER_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5_000),
    )
}

/// The background sweep: detect **merged** changes (a change landed on the
/// canonical branch) and append `lifecycle{merged}` entries
/// (docs/data-model.md "Lifecycle"). The only writer of `merged`. It never
/// abandons — abandonment is an explicit action (`abandon_change`).
async fn run_lifecycle_timer(state: Arc<AppState>) {
    let interval = timer_interval();
    let mut shutdown = state.shutdown_watch();
    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => {}
            _ = shutdown.wait_for(|&s| s) => break,
        }
        let st = state.clone();
        let _ = blocking(move || {
            sweep_lifecycle(&st);
            Ok(())
        })
        .await;
    }
}

fn sweep_lifecycle(state: &Arc<AppState>) {
    for repo_id in state.repo_ids() {
        let Some(repo_state) = state.repo_state(repo_id) else {
            continue;
        };
        let Ok(repo) = Repository::open(repo_state.git_dir()) else {
            continue;
        };
        let view = state.repo_view(repo_id);
        for change_id in live_change_ids(&view) {
            let Some(entry) = state.change_entry(change_id) else {
                continue;
            };
            let snapshot = entry.read().clone();
            if let Some(landed) =
                gitscan::landed_revision(&repo, &repo_state.base_branch, &snapshot)
            {
                append_lifecycle(
                    state,
                    &entry,
                    change_id,
                    LifecycleAction::Merged,
                    Some(landed),
                );
            }
        }
    }
}

/// Change ids in `view` that are not terminal (the timer's working set).
fn live_change_ids(view: &RepoView) -> Vec<u64> {
    view.change_ids()
        .into_iter()
        .filter(|id| view.change(*id).is_some_and(|c| !c.is_terminal()))
        .collect()
}

fn append_lifecycle(
    state: &Arc<AppState>,
    entry: &ChangeEntry,
    change_id: u64,
    action: LifecycleAction,
    revision: Option<u64>,
) {
    let payload = match serde_json::to_value(review::LifecyclePayload {
        action,
        revision,
        message: None,
    }) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("lifecycle payload: {e}");
            return;
        }
    };
    let Ok(mut conn) = state.open_db() else {
        return;
    };
    if let Err(e) = append_to_change(
        &mut conn,
        entry,
        change_id,
        vec![(LogKind::Lifecycle, payload)],
    ) {
        tracing::warn!(change_id, "lifecycle append failed: {e:#}");
    }
}
