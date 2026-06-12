//! Git object plumbing for the scan: patch-ids and the GC-safety keep
//! refs (docs/data-model.md "GC safety").

use anyhow::Result;
use git2::{Commit, Oid, Repository, Tree};

use crate::db;

/// Patch-id of the empty diff: the sha1 of the empty string
/// (docs/data-model.md identity rule 3 sentinel).
pub const EMPTY_PATCH_ID: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";

/// `git patch-id --stable`-equivalent id of the diff `old → new`.
///
/// # Errors
/// When git can't diff the trees or compute the patch-id.
pub fn tree_patch_id(repo: &Repository, old: &Tree, new: &Tree) -> Result<String> {
    let diff = repo.diff_tree_to_tree(Some(old), Some(new), None)?;
    if diff.deltas().len() == 0 {
        return Ok(EMPTY_PATCH_ID.to_string());
    }
    Ok(diff.patchid(None)?.to_string())
}

/// Patch-id of a commit against its first parent.
///
/// # Errors
/// When `commit` has no first parent or the diff fails.
pub fn commit_patch_id(repo: &Repository, commit: &Commit) -> Result<String> {
    let parent_tree = commit.parent(0)?.tree()?;
    tree_patch_id(repo, &parent_tree, &commit.tree()?)
}

/// Ref name pinning one revision's git objects against `git gc`.
#[must_use]
pub fn keep_ref_name(chain_id: i64, change_id: i64, revision_number: i64) -> String {
    format!("refs/nit/keep/{chain_id}/{change_id}/{revision_number}")
}

/// Ensure the keep ref for a revision exists, pointing at the revision's
/// commit — its parent (the diff's old side) is reachable through it.
/// Best-effort: failures (e.g. objects already pruned) are logged, never
/// fatal.
pub fn ensure_keep_ref(repo: &Repository, chain_id: i64, change_id: i64, rev: &db::Revision) {
    if let Err(err) = try_ensure_keep_ref(repo, chain_id, change_id, rev) {
        tracing::warn!(
            chain_id,
            change_id,
            revision = rev.number,
            "cannot maintain keep ref: {err:#}"
        );
    }
}

fn try_ensure_keep_ref(
    repo: &Repository,
    chain_id: i64,
    change_id: i64,
    rev: &db::Revision,
) -> Result<()> {
    let name = keep_ref_name(chain_id, change_id, rev.number);
    let oid = Oid::from_str(&rev.commit_sha)?;
    let current = repo.find_reference(&name).ok().and_then(|r| r.target());
    if current != Some(oid) {
        // Writing the ref validates the target object exists.
        repo.reference(&name, oid, true, "nit: keep")?;
    }
    Ok(())
}

/// Drop every keep ref of a chain — run by the scan that closes it
/// (merged/abandoned). Best-effort.
pub fn delete_chain_keep_refs(repo: &Repository, chain_id: i64) {
    let glob = format!("refs/nit/keep/{chain_id}/*");
    let Ok(mut refs) = repo.references_glob(&glob) else {
        return;
    };
    let names: Vec<String> = refs.names().flatten().map(str::to_string).collect();
    for name in names {
        if let Ok(mut reference) = repo.find_reference(&name)
            && let Err(err) = reference.delete()
        {
            tracing::warn!(chain_id, name, "cannot delete keep ref: {err}");
        }
    }
}
