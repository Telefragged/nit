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
/// Keyed on the change, so a commit a prefix-merged ancestor still walks
/// through keeps its objects.
///
/// The refs of a change live as long as the change. Dropping a ref
/// earlier can orphan objects that the sha-walk, a vs-parent diff of
/// retained history, or the timer's `fork_sha..canonical` walk still needs.
#[must_use]
pub fn keep_ref_name(change_number: ChangeNumber, revision_number: RevisionNumber) -> String {
    format!("{}{revision_number}", keep_ref_prefix(change_number))
}

/// The prefix of the keep refs of every revision of a change.
#[must_use]
pub fn keep_ref_prefix(change_number: ChangeNumber) -> String {
    format!("refs/nit/keep/{change_number}/")
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

/// Deletes the keep refs of every revision of a change.
///
/// This is best-effort, like [`ensure_keep_ref`]. On a failure it logs a
/// warning and leaves the remaining refs in place. Those refs still pin
/// their objects.
pub fn delete_keep_refs(repo: &Repository, change_number: ChangeNumber) {
    let glob = format!("{}*", keep_ref_prefix(change_number));
    let deleted = repo.references_glob(&glob).and_then(|refs| {
        refs.collect::<Result<Vec<_>, _>>()?
            .iter_mut()
            .try_for_each(git2::Reference::delete)
    });
    if let Err(err) = deleted {
        tracing::warn!(
            change_number = change_number.get(),
            "cannot delete keep refs: {err:#}"
        );
    }
}
