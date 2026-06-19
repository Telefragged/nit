//! Git object plumbing: patch-ids and the GC-safety keep refs
//! (docs/data-model.md "Keep refs").

use anyhow::Result;
use git2::{Commit, Oid, Repository, Tree};

/// Patch-id of the empty diff: the sha1 of the empty string (the
/// merged-test sentinel for trivially-matched empty diffs).
pub const EMPTY_PATCH_ID: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";

/// `git patch-id --stable`-equivalent id of the diff `old → new`.
///
/// # Errors
/// When git can't diff the trees or compute the patch-id.
fn tree_patch_id(repo: &Repository, old: &Tree, new: &Tree) -> Result<String> {
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
fn commit_patch_id(repo: &Repository, commit: &Commit) -> Result<String> {
    let parent_tree = commit.parent(0)?.tree()?;
    tree_patch_id(repo, &parent_tree, &commit.tree()?)
}

/// [`commit_patch_id`] for the commit `sha` names; `None` when anything
/// is unresolvable.
#[must_use]
pub fn sha_patch_id(repo: &Repository, sha: &str) -> Option<String> {
    let commit = repo.find_commit(Oid::from_str(sha).ok()?).ok()?;
    commit_patch_id(repo, &commit).ok()
}

/// Ref name pinning one revision's git objects against `git gc`. Keyed on the
/// change (a chain is not stored), so a commit a prefix-merged ancestor still
/// walks through keeps its objects.
#[must_use]
pub fn keep_ref_name(change_id: u64, revision_number: u64) -> String {
    format!("refs/nit/keep/{change_id}/{revision_number}")
}

/// Ensure the keep ref for a revision exists, pointing at the revision's
/// commit — its parent (the diff's old side) is reachable through it.
/// Best-effort: failures (e.g. objects already pruned) are logged, never
/// fatal.
pub fn ensure_keep_ref(repo: &Repository, change_id: u64, number: u64, commit_sha: &str) {
    if let Err(err) = try_ensure_keep_ref(repo, change_id, number, commit_sha) {
        tracing::warn!(
            change_id,
            revision = number,
            "cannot maintain keep ref: {err:#}"
        );
    }
}

fn try_ensure_keep_ref(
    repo: &Repository,
    change_id: u64,
    number: u64,
    commit_sha: &str,
) -> Result<()> {
    let name = keep_ref_name(change_id, number);
    let oid = Oid::from_str(commit_sha)?;
    let current = repo.find_reference(&name).ok().and_then(|r| r.target());
    if current != Some(oid) {
        // Writing the ref validates the target object exists.
        repo.reference(&name, oid, true, "nit: keep")?;
    }
    Ok(())
}
