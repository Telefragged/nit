//! Repo endpoints: create, list, fetch, relocate, retarget, and remove
//! registered repos.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use git2::Repository;
use rusqlite::Connection;

use nit_types::domain::Sha;
use nit_types::repos::{CreateRepo, RelocateRepo, Repo, RepoList, SetCanonicalRef};

use crate::db;
use crate::gitscan;
use crate::gitscan::objects;

use super::canonical_git_dir;
use super::{AppJson, AppPath, AppState, Appends, Error, with_conn};

fn repo_json(conn: &Connection, row: db::RepoRow) -> Result<Repo, Error> {
    Ok(Repo {
        id: row.id,
        git_dir: row.git_dir,
        canonical_ref: row.canonical_ref,
        open_changes: db::count_changes(conn, row.id, &db::ChangeFilter::open())?,
    })
}

/// Resolves `canonical_ref` to the commit it names (400 if it names none).
fn resolve_ref(repo: &Repository, canonical_ref: &str) -> Result<Sha, Error> {
    let commit = repo
        .revparse_single(canonical_ref)
        .and_then(|o| o.peel_to_commit())
        .map_err(|e| {
            Error::bad_request(format!(
                "cannot resolve '{canonical_ref}' to a commit — name an existing git ref: {}",
                e.message()
            ))
        })?;
    Ok(gitscan::sha_of(commit.id()))
}

/// Registers a repo (`nit repo create`) with its canonical ref.
///
/// `canonical_ref` must resolve to a commit
/// — any git ref, e.g. `origin/main` (400 otherwise); nit never guesses it.
/// 409 if the git dir is already registered.
#[utoipa::path(post, path = "/api/repos", request_body = CreateRepo, responses((status = 200, body = Repo)))]
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
        let head = resolve_ref(&repo, &req.canonical_ref)?;
        // The row and its baseline are one registration. The baseline seeds
        // the merge timer at the canonical ref's current HEAD, so the first
        // merge after registration shows up in a delta scan rather than
        // falling into pre-tracking history. Two commits would lose exactly
        // that on a crash between them.
        let row = db::write(conn, |tx| {
            let row = db::create_repo(tx, &canonical, &req.canonical_ref)?;
            db::update_repo_canonical_head(tx, row.id, &head)?;
            Ok(row)
        })?;
        Ok(Json(repo_json(conn, row)?))
    })
    .await
}

/// Lists every registered repo with its open-change count (derived, never stored).
#[utoipa::path(get, path = "/api/repos", responses((status = 200, body = RepoList)))]
pub(super) async fn list_repos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RepoList>, Error> {
    with_conn(state.pool(), move |conn| {
        let repos = db::all_repos(conn)?
            .into_iter()
            .map(|r| repo_json(conn, r))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Json(RepoList { repos }))
    })
    .await
}

/// One repo by id; open-change count derived, never stored (404 if unknown).
#[utoipa::path(get, path = "/api/repos/{id}", params(("id" = u64, Path)), responses((status = 200, body = Repo)))]
pub(super) async fn get_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
) -> Result<Json<Repo>, Error> {
    with_conn(state.pool(), move |conn| {
        let row = db::get_repo(conn, repo_id)?
            .ok_or_else(|| Error::not_found(format!("repo {repo_id} not found")))?;
        Ok(Json(repo_json(conn, row)?))
    })
    .await
}

/// Repoint a repo at a new git-common-dir after it moved on disk.
#[utoipa::path(patch, path = "/api/repos/{id}", params(("id" = u64, Path)), request_body = RelocateRepo, responses((status = 200, body = Repo)))]
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
        db::write(conn, |tx| db::update_repo_git_dir(tx, repo_id, &canonical))?;
        let row = db::RepoRow {
            git_dir: canonical,
            ..existing
        };
        Ok(Json(repo_json(conn, row)?))
    })
    .await
}

/// Points a repo at a new canonical ref (`nit repo set-ref`).
///
/// `nit_types::repos::SetCanonicalRef` carries the contract. 409 if the new
/// ref resolves to another commit, 400 if it resolves to none, 404 if the
/// repo is unknown.
#[utoipa::path(put, path = "/api/repos/{id}/canonical-ref", params(("id" = u64, Path)), request_body = SetCanonicalRef, responses((status = 200, body = Repo)))]
pub(super) async fn set_canonical_ref(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
    AppJson(req): AppJson<SetCanonicalRef>,
) -> Result<Json<Repo>, Error> {
    with_conn(state.pool(), move |conn| {
        let existing = db::get_repo(conn, repo_id)?
            .ok_or_else(|| Error::not_found(format!("repo {repo_id} not found")))?;
        let repo = Repository::open(&existing.git_dir)
            .map_err(|e| Error::internal(format!("cannot open repository: {e}")))?;
        let head = resolve_ref(&repo, &req.canonical_ref)?;
        if !db::write(conn, |tx| {
            db::update_repo_canonical_ref(tx, repo_id, &req.canonical_ref, &head)
        })? {
            let recorded = existing
                .canonical_head
                .as_ref()
                .map_or_else(|| "no commit yet".to_string(), gitscan::short_sha);
            return Err(Error::conflict(format!(
                "'{}' resolves to {}, but nit recorded '{}' at {recorded} — the new ref must point at the same commit",
                req.canonical_ref,
                gitscan::short_sha(&head),
                existing.canonical_ref,
            )));
        }
        let row = db::RepoRow {
            canonical_ref: req.canonical_ref,
            ..existing
        };
        Ok(Json(repo_json(conn, row)?))
    })
    .await
}

/// Deletes a repo and each of its changes (404 if unknown).
///
/// The attachments of the deleted changes stay until the next `POST
/// /api/gc`.
#[utoipa::path(delete, path = "/api/repos/{id}", params(("id" = u64, Path)), responses((status = 204)))]
pub(super) async fn remove_repo(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
) -> Result<StatusCode, Error> {
    with_conn(state.pool(), move |conn| {
        let not_found = || Error::not_found(format!("repo {repo_id} not found"));
        let row = db::get_repo(conn, repo_id)?.ok_or_else(not_found)?;
        let mut appends = Appends::begin(conn)?;
        if !appends.delete_repo(repo_id)? {
            return Err(not_found());
        }
        appends.commit(&state)?;
        // This deletes the refs only after the commit. A crash between the
        // two can leave extra refs that pin objects. It never leaves a
        // change without its objects.
        if let Ok(repo) = Repository::open(&row.git_dir) {
            objects::delete_all_keep_refs(&repo);
        }
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
