//! Git object plumbing for the scan: patch-ids, effective-tree folding
//! (scan step 5) and the GC-safety keep refs (docs/data-model.md "GC
//! safety").

use anyhow::Result;
use git2::{Commit, Oid, Repository, Tree};

use crate::db;

/// Patch-id of the empty diff: the sha1 of the empty string
/// (docs/data-model.md identity rule 3 sentinel).
pub const EMPTY_PATCH_ID: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";

/// `git patch-id --stable`-equivalent id of the diff `old → new`.
pub fn tree_patch_id(repo: &Repository, old: &Tree, new: &Tree) -> Result<String> {
    let diff = repo.diff_tree_to_tree(Some(old), Some(new), None)?;
    if diff.deltas().len() == 0 {
        return Ok(EMPTY_PATCH_ID.to_string());
    }
    Ok(diff.patchid(None)?.to_string())
}

/// Patch-id of a commit against its first parent.
pub fn commit_patch_id(repo: &Repository, commit: &Commit) -> Result<String> {
    let parent_tree = commit.parent(0)?.tree()?;
    tree_patch_id(repo, &parent_tree, &commit.tree()?)
}

/// Fold `fixups` (branch order) into `commit`'s tree by iterated in-memory
/// three-way merge: ancestor = the fixup's parent tree, ours = the
/// accumulated tree, theirs = the fixup's tree. `Ok(None)` = fold conflict
/// (`effective_tree` NULL, `needs_rebase`). Merged trees are written to
/// the repository odb so they outlive the scan.
pub fn effective_tree(
    repo: &Repository,
    commit: &Commit,
    fixups: &[&Commit],
) -> Result<Option<Oid>> {
    let mut ours = commit.tree()?;
    for fixup in fixups {
        let ancestor = fixup.parent(0)?.tree()?;
        let theirs = fixup.tree()?;
        let mut index = repo.merge_trees(&ancestor, &ours, &theirs, None)?;
        if index.has_conflicts() {
            return Ok(None);
        }
        let oid = index.write_tree_to(repo)?;
        ours = repo.find_tree(oid)?;
    }
    Ok(Some(ours.id()))
}

/// Ref name pinning one revision's git objects against `git gc`.
#[must_use]
pub fn keep_ref_name(chain_id: i64, change_id: i64, revision_number: i64) -> String {
    format!("refs/nit/keep/{chain_id}/{change_id}/{revision_number}")
}

/// Ensure the keep ref for a revision exists: a synthetic commit whose
/// tree is the effective tree (the original commit's tree when folding
/// conflicted) and whose parents are `[parent, original, fixups…]` —
/// making parent, original, fold *and* the folded fixup commits reachable
/// (the fixups are needed for later pure-rebase comparisons and re-folds).
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
    let parent = repo.find_commit(Oid::from_str(&rev.parent_sha)?)?;
    let original = repo.find_commit(Oid::from_str(&rev.commit_sha)?)?;
    let tree = match &rev.effective_tree {
        Some(t) => repo.find_tree(Oid::from_str(t)?)?,
        None => original.tree()?,
    };
    let mut parents = vec![parent, original];
    for fixup in &rev.fixups {
        parents.push(repo.find_commit(Oid::from_str(&fixup.sha)?)?);
    }
    // Deterministic signature: recreating the synthetic commit yields the
    // same oid, so repeated scans are no-ops.
    let sig = git2::Signature::new("nit", "nit@localhost", &git2::Time::new(0, 0))?;
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    let oid = repo.commit(
        None,
        &sig,
        &sig,
        "nit: pin review objects (parent, original, fold, fixups)",
        &tree,
        &parent_refs,
    )?;
    let current = repo.find_reference(&name).ok().and_then(|r| r.target());
    if current != Some(oid) {
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
