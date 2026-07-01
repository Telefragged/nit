//! Change endpoints: the change detail and the revision diff (incl. interdiff).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::{Oid, Repository, Tree};
use serde::Deserialize;

use nit_types::changes::{ChangeDetail, ChangeDrafts};
use nit_types::diff::{Diff, FileLines};

use crate::review;

use super::diff;
use super::rebase;
use super::views;
use super::{AppPath, AppQuery, AppState, ChangeEntry, Error, with_conn};
use super::{change_detail_json, change_or_404};

pub(super) async fn get_change_detail(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        change_detail_json(conn, &entry)
    })
    .await
}

/// `GET /api/changes/{id}/drafts` — the reviewer's private overlay (drafts +
/// staged decision). The change page reads this over REST and the folded
/// projection over the websocket (docs/api.md "Events").
pub(super) async fn get_change_drafts(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<ChangeDrafts>, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(&state, conn, id)?;
        Ok(Json(views::change_overlay(conn, id)?))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct DiffQuery {
    against: Option<u64>,
}

pub(super) async fn revision_diff(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(u64, u64)>,
    AppQuery(q): AppQuery<DiffQuery>,
) -> Result<Json<Diff>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let revs = resolve_revs(&state, &entry, n, q.against)?;
        let repo = open_repo(&revs.git_dir)?;
        let new_tree = commit_tree(&repo, &revs.rev.commit_sha)?;
        let (old_tree, against_message, against_rev) = old_side(&repo, &revs)?;
        let mut wire = diff::diff_trees(&repo, &old_tree, &new_tree)?;
        wire.files.insert(
            0,
            diff::commit_msg_file(against_message.as_deref(), &revs.rev.message)?,
        );
        tag_interdiff_drift(&repo, &mut wire, &revs.rev, against_rev.as_ref());
        Ok(Json(wire))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct LinesQuery {
    path: String,
    against: Option<u64>,
}

/// File `path`'s full-context diff lines, so the UI can reveal the unchanged
/// runs the shown diff hides. Built from the **same** `old → new` trees and
/// drift tagging as [`revision_diff`], so a revealed line carries the exact
/// kind/drift it would inside a hunk; the client slices the gap it needs.
pub(super) async fn revision_lines(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(u64, u64)>,
    AppQuery(q): AppQuery<LinesQuery>,
) -> Result<Json<FileLines>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, id)?;
        let revs = resolve_revs(&state, &entry, n, q.against)?;
        let repo = open_repo(&revs.git_dir)?;
        let new_tree = commit_tree(&repo, &revs.rev.commit_sha)?;
        let (old_tree, _, against_rev) = old_side(&repo, &revs)?;
        let mut wire = diff::diff_trees_full(&repo, &old_tree, &new_tree, &q.path)?;
        tag_interdiff_drift(&repo, &mut wire, &revs.rev, against_rev.as_ref());
        let lines = wire
            .files
            .into_iter()
            .next()
            .map(|f| f.hunks.into_iter().flat_map(|h| h.lines).collect())
            .unwrap_or_default();
        Ok(Json(FileLines { lines }))
    })
    .await
}

/// A revision and an optional interdiff counterpart, cloned out from under
/// the projection read lock so the git work holds nothing live.
struct Revs {
    git_dir: String,
    rev: review::RevisionProj,
    against: Option<review::RevisionProj>,
}

fn resolve_revs(
    state: &AppState,
    entry: &ChangeEntry,
    n: u64,
    against: Option<u64>,
) -> Result<Revs, Error> {
    let proj = entry.read();
    let find = |k: u64| {
        proj.revision(k)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("revision {k} not found")))
    };
    Ok(Revs {
        git_dir: state.git_dir(proj.repo_id)?,
        rev: find(n)?,
        against: against.map(find).transpose()?,
    })
}

/// The diff's old side, plus (for an interdiff) the FROM message and the
/// `(m_sha, parent_m)` that [`tag_interdiff_drift`] needs.
type AgainstRev = (String, String);
fn old_side<'r>(
    repo: &'r Repository,
    revs: &Revs,
) -> Result<(Tree<'r>, Option<String>, Option<AgainstRev>), Error> {
    match &revs.against {
        None => {
            let parent = repo
                .find_commit(parse_oid(&revs.rev.parent_sha)?)
                .map_err(|e| Error::internal(format!("parent commit missing: {e}")))?;
            let tree = parent
                .tree()
                .map_err(|e| Error::internal(format!("parent tree missing: {e}")))?;
            Ok((tree, None, None))
        }
        Some(a) => Ok((
            commit_tree(repo, &a.commit_sha)?,
            Some(a.message.clone()),
            Some((a.commit_sha.clone(), a.parent_sha.clone())),
        )),
    }
}

/// Contain rebase drift in an interdiff whose two revisions have different
/// parents (docs/api.md "Rebase-aware interdiffs"); a no-op otherwise. Best
/// effort: on failure the plain interdiff is served.
fn tag_interdiff_drift(
    repo: &Repository,
    wire: &mut Diff,
    rev: &review::RevisionProj,
    against: Option<&AgainstRev>,
) {
    if let Some((m_sha, parent_m)) = against
        && *parent_m != rev.parent_sha
        && let Err(e) = rebase::tag_drift(
            repo,
            wire,
            m_sha,
            parent_m,
            &rev.commit_sha,
            &rev.parent_sha,
        )
    {
        tracing::warn!("rebase-aware interdiff tagging failed; serving plain interdiff: {e:#}");
    }
}

fn open_repo(git_dir: &str) -> Result<Repository, Error> {
    Repository::open(git_dir)
        .map_err(|e| Error::internal(format!("cannot open the repository: {e}")))
}

fn commit_tree<'r>(repo: &'r Repository, sha: &str) -> Result<Tree<'r>, Error> {
    diff::commit_tree(repo, sha).ok_or_else(|| Error::internal(format!("tree for {sha} missing")))
}

fn parse_oid(sha: &str) -> Result<Oid, Error> {
    Oid::from_str(sha).map_err(|e| Error::internal(format!("bad sha {sha:?}: {e}")))
}
