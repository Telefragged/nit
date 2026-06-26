//! Repo endpoints: create, list, fetch, and relocate registered repos.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;

use crate::db;

use super::canonical_git_dir;
use super::types;
use super::{AppJson, AppPath, AppState, Error, with_conn};

/// A repo row plus its derived live-tip count, as the wire `Repo`.
fn repo_json(state: &AppState, row: db::RepoRow) -> types::Repo {
    let active = u64::try_from(state.repo_view(row.id).tips().len()).unwrap_or(u64::MAX);
    types::Repo {
        id: row.id,
        git_dir: row.git_dir,
        base_ref: row.base_ref,
        active_chains: active,
    }
}

/// Register a repo (`nit repo create`), configuring its one canonical base
/// ref. `base` must resolve to a commit — any git ref, e.g. `origin/main`
/// (400 otherwise); nit never guesses it. 409 if the git dir is already
/// registered.
pub(super) async fn create_repo(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<types::CreateRepo>,
) -> Result<Json<types::Repo>, Error> {
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
        // Resolve the base to a commit up front — any git ref (a local branch,
        // `origin/main`, a tag, a sha), not only a local branch. This both
        // validates it (400 otherwise) and seeds the merge timer's baseline
        // below; nit never guesses the base.
        let base_commit = repo
            .revparse_single(&req.base)
            .and_then(|o| o.peel_to_commit())
            .map_err(|e| {
                Error::bad_request(format!(
                    "cannot resolve '{}' to a commit — name an existing git ref as the base: {}",
                    req.base,
                    e.message()
                ))
            })?;
        let row = db::create_repo(conn, &canonical, &req.base)?;
        // Seed the merge timer's baseline at the base ref's current HEAD, so the
        // first landing after registration shows up in a delta scan rather than
        // being swallowed as pre-tracking history (docs/data-model.md).
        db::update_repo_base_head(conn, row.id, &base_commit.id().to_string())?;
        state.ensure_repo(&row);
        Ok(Json(repo_json(&state, row)))
    })
    .await
}

/// List every registered repo with its live-tip count (derived, never stored).
pub(super) async fn list_repos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<types::RepoList>, Error> {
    with_conn(state.pool(), move |conn| {
        let repos = db::all_repos(conn)?
            .into_iter()
            .map(|r| repo_json(&state, r))
            .collect();
        Ok(Json(types::RepoList { repos }))
    })
    .await
}

/// One repo by id, with its live-tip count (404 if unknown).
pub(super) async fn get_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
) -> Result<Json<types::Repo>, Error> {
    with_conn(state.pool(), move |conn| {
        let row = db::get_repo(conn, repo_id)?
            .ok_or_else(|| Error::not_found(format!("repo {repo_id} not found")))?;
        Ok(Json(repo_json(&state, row)))
    })
    .await
}

/// Repoint a repo at a new git-common-dir after it moved on disk.
pub(super) async fn relocate_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
    AppJson(req): AppJson<types::RelocateRepo>,
) -> Result<Json<types::Repo>, Error> {
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
            base_ref: existing.base_ref,
            base_head: existing.base_head,
        };
        state.ensure_repo(&row);
        Ok(Json(repo_json(&state, row)))
    })
    .await
}
