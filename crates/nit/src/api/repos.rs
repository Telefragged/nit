//! Repo endpoints: list, fetch, and relocate registered repos.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;

use crate::db;

use super::canonical_git_dir;
use super::types;
use super::{AppJson, AppPath, AppState, Error, blocking};

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
pub(super) async fn list_repos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<types::RepoList>, Error> {
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

/// One repo by id, with its live-tip count (404 if unknown).
pub(super) async fn get_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
) -> Result<Json<types::Repo>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let row = db::get_repo(&conn, repo_id)?
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
