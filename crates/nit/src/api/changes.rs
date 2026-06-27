//! Change endpoints: the change detail and the revision diff (incl. interdiff).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::{Oid, Repository};
use serde::Deserialize;

use nit_types::changes::ChangeDetail;
use nit_types::diff::Diff;

use crate::review;

use super::diff;
use super::rebase;
use super::{AppPath, AppQuery, AppState, Error, with_conn};
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
        // Clone the revision(s) out from under the read lock so the git work
        // below holds nothing live (a diff is not a hot path).
        let (git_dir, rev, against): (String, review::RevisionProj, Option<review::RevisionProj>) = {
            let proj = entry.read();
            let rev = proj
                .revision(n)
                .cloned()
                .ok_or_else(|| Error::not_found(format!("revision {n} not found")))?;
            let against = q
                .against
                .map(|m| {
                    proj.revision(m)
                        .cloned()
                        .ok_or_else(|| Error::not_found(format!("revision {m} not found")))
                })
                .transpose()?;
            let git_dir = state.git_dir(proj.repo_id)?;
            (git_dir, rev, against)
        };
        let repo = Repository::open(&git_dir)
            .map_err(|e| Error::internal(format!("cannot open the repository: {e}")))?;
        let new_tree = commit_tree(&repo, &rev.commit_sha)?;
        let (old_tree, against_message, against_rev) = match against {
            None => {
                let parent = repo
                    .find_commit(parse_oid(&rev.parent_sha)?)
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
            diff::commit_msg_file(against_message.as_deref(), &rev.message)?,
        );
        if let Some((m_sha, parent_m)) = against_rev
            && parent_m != rev.parent_sha
            && let Err(e) =
                rebase::tag_drift(&repo, &mut wire, &m_sha, &parent_m, &rev.commit_sha, &rev.parent_sha)
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
