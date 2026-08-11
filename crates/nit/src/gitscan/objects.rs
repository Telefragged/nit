//! Git object plumbing: patch-ids and the GC-safety keep refs.

use anyhow::Result;
use git2::{Commit, Oid, Repository, Tree};
use nit_types::domain::ChangeNumber;
use nit_types::domain::RevisionNumber;
use nit_types::domain::Sha;

/// Patch-id of the empty diff: the sha1 of the empty string.
const EMPTY_PATCH_ID: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";

/// `git patch-id --stable`-equivalent id of the diff `old → new`.
///
/// # Errors
///
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
///
/// When `commit` has no first parent or the diff fails.
fn commit_patch_id(repo: &Repository, commit: &Commit) -> Result<String> {
    let parent_tree = commit.parent(0)?.tree()?;
    tree_patch_id(repo, &parent_tree, &commit.tree()?)
}

/// The patch id of the commit `sha` names.
///
/// Its diff against its first parent, whitespace-normalized.
#[must_use]
pub fn sha_patch_id(repo: &Repository, sha: &Sha) -> Option<String> {
    let commit = repo.find_commit(Oid::from_str(sha.as_str()).ok()?).ok()?;
    commit_patch_id(repo, &commit).ok()
}

/// Ref name pinning one revision's git objects against `git gc`.
///
/// Keyed on the change (a chain is not stored), so a commit a
/// prefix-merged ancestor still walks through keeps its objects.
///
/// Deleting these refs is deferred on purpose — nothing prunes them, even
/// for merged/abandoned changes. Over-pinning is fail-safe; dropping a ref
/// can orphan objects the sha-walk, a vs-parent diff of retained history,
/// or the timer's `fork_sha..canonical` walk still needs.
#[must_use]
pub fn keep_ref_name(change_number: ChangeNumber, revision_number: RevisionNumber) -> String {
    format!("refs/nit/keep/{change_number}/{revision_number}")
}

/// Ensures the keep ref for a revision exists.
///
/// Points it at the revision's commit — its parent (the diff's old side)
/// is reachable through it. Best-effort: failures (e.g. objects already
/// pruned) are logged, never fatal.
pub fn ensure_keep_ref(
    repo: &Repository,
    change_number: ChangeNumber,
    number: RevisionNumber,
    commit_sha: &Sha,
) {
    if let Err(err) = try_ensure_keep_ref(repo, change_number, number, commit_sha) {
        tracing::warn!(
            change_number = change_number.get(),
            revision = number.get(),
            "cannot maintain keep ref: {err:#}"
        );
    }
}

fn try_ensure_keep_ref(
    repo: &Repository,
    change_number: ChangeNumber,
    number: RevisionNumber,
    commit_sha: &Sha,
) -> Result<()> {
    let name = keep_ref_name(change_number, number);
    let oid = Oid::from_str(commit_sha.as_str())?;
    let current = repo.find_reference(&name).ok().and_then(|r| r.target());
    if current != Some(oid) {
        // Writing the ref validates the target object exists.
        repo.reference(&name, oid, true, "nit: keep")?;
    }
    Ok(())
}
