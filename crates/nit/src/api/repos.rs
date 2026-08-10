//! Repo endpoints: create, list, fetch, and relocate registered repos.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;
use rusqlite::Connection;

use nit_types::repos::{CreateRepo, RelocateRepo, Repo, RepoList};

use crate::db;

use super::canonical_git_dir;
use super::{AppJson, AppPath, AppState, Error, with_conn};

fn repo_json(state: &AppState, conn: &Connection, row: db::RepoRow) -> Result<Repo, Error> {
    let tips = state.repo_view(conn, row.id)?.tips().len();
    Ok(Repo {
        id: row.id,
        git_dir: row.git_dir,
        canonical_ref: row.canonical_ref,
        active_chains: u64::try_from(tips).unwrap_or(u64::MAX),
    })
}

/// Registers a repo (`nit repo create`) with its canonical ref.
///
/// `canonical_ref` must resolve to a commit
/// — any git ref, e.g. `origin/main` (400 otherwise); nit never guesses it.
/// 409 if the git dir is already registered.
pub(super) async fn create_repo(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<CreateRepo>,
) -> Result<Json<Repo>, Error> {
    with_conn(state.pool(), move |conn| {
        let canonical = canonical_git_dir(&req.git_dir)?;
        let repo = Repository::open(&canonical).map_err(|e| {
            Error::bad_request(format!(
                "not a git repository at {canonical}: {}",
                e.message()
            ))
        })?;
        if let Some(existing) = db::find_repo(conn, &canonical)? {
            return Err(Error::conflict(format!(
                "{canonical} is already registered as repo {}",
                existing.id
            )));
        }
        let base_commit = repo
            .revparse_single(&req.canonical_ref)
            .and_then(|o| o.peel_to_commit())
            .map_err(|e| {
                Error::bad_request(format!(
                    "cannot resolve '{}' to a commit — name an existing git ref: {}",
                    req.canonical_ref,
                    e.message()
                ))
            })?;
        let row = db::create_repo(conn, &canonical, &req.canonical_ref)?;
        // Seed the merge timer's baseline at the canonical ref's current HEAD, so the
        // first merge after registration shows up in a delta scan rather than
        // being swallowed as pre-tracking history.
        db::update_repo_canonical_head(conn, row.id, &base_commit.id().to_string())?;
        state.ensure_repo(&row);
        Ok(Json(repo_json(&state, conn, row)?))
    })
    .await
}

/// Lists every registered repo with its live-tip count (derived, never stored).
pub(super) async fn list_repos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RepoList>, Error> {
    with_conn(state.pool(), move |conn| {
        let repos = db::all_repos(conn)?
            .into_iter()
            .map(|r| repo_json(&state, conn, r))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Json(RepoList { repos }))
    })
    .await
}

/// One repo by id; live-tip count derived, never stored (404 if unknown).
pub(super) async fn get_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
) -> Result<Json<Repo>, Error> {
    with_conn(state.pool(), move |conn| {
        let row = db::get_repo(conn, repo_id)?
            .ok_or_else(|| Error::not_found(format!("repo {repo_id} not found")))?;
        Ok(Json(repo_json(&state, conn, row)?))
    })
    .await
}

/// Repoint a repo at a new git-common-dir after it moved on disk.
pub(super) async fn relocate_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
    AppJson(req): AppJson<RelocateRepo>,
) -> Result<Json<Repo>, Error> {
    with_conn(state.pool(), move |conn| {
        let existing = db::get_repo(conn, repo_id)?
            .ok_or_else(|| Error::not_found(format!("repo {repo_id} not found")))?;
        let canonical = canonical_git_dir(&req.git_dir)?;
        Repository::open(&canonical).map_err(|e| {
            Error::bad_request(format!(
                "not a git repository at {canonical}: {}",
                e.message()
            ))
        })?;
        if let Some(other) = db::find_repo(conn, &canonical)?
            && other.id != repo_id
        {
            return Err(Error::conflict(format!(
                "git dir {canonical} is already registered as repo {}",
                other.id
            )));
        }
        db::update_repo_git_dir(conn, repo_id, &canonical)?;
        let row = db::RepoRow {
            id: repo_id,
            git_dir: canonical,
            canonical_ref: existing.canonical_ref,
            canonical_head: existing.canonical_head,
        };
        state.ensure_repo(&row);
        Ok(Json(repo_json(&state, conn, row)?))
    })
    .await
}
