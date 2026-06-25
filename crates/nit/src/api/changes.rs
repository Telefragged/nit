//! Change endpoints: the change detail and the revision diff (incl. interdiff).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::{Oid, Repository};
use serde::Deserialize;

use super::diff;
use super::rebase;
use super::types;
use super::views;
use super::{AppPath, AppQuery, AppState, Error, blocking};
use super::{change_detail_json, change_or_404};

pub(super) async fn get_change_detail(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::ChangeDetail>, Error> {
    blocking(move || {
        let conn = state.open_db()?;
        let entry = change_or_404(&state, id)?;
        change_detail_json(&conn, &entry)
    })
    .await
}

/// `GET /api/changes/{id}/chains` — every tip walking through this change, each
/// pinned to the patchset it walks (docs/api.md "Changes"). Derived from a repo
/// view, kept separate from the change detail so a change read builds no view.
pub(super) async fn change_chains(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<Json<types::ChainsThrough>, Error> {
    blocking(move || {
        let entry = change_or_404(&state, id)?;
        let repo_id = entry.read().repo_id;
        let view = state.repo_view(repo_id);
        let chains = views::chains_through_view(&view, id);
        Ok(Json(types::ChainsThrough { chains }))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct DiffQuery {
    against: Option<u64>,
}

struct AgainstRev {
    commit_sha: String,
    message: String,
    parent_sha: String,
}

pub(super) async fn revision_diff(
    State(state): State<Arc<AppState>>,
    AppPath((id, n)): AppPath<(u64, u64)>,
    AppQuery(q): AppQuery<DiffQuery>,
) -> Result<Json<types::Diff>, Error> {
    blocking(move || {
        let entry = change_or_404(&state, id)?;
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
