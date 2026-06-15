//! Git scan engine: reconciles a registered branch (`base..tip`) with a
//! chain's [`Projection`] — docs/data-model.md "Scan algorithm" is the
//! contract.
//!
//! [`scan`] is pure with respect to the database: it reads the current
//! projection plus the repo and returns a [`ScanResult`] — the entries to
//! append (a `revisions` and/or `chain_closed`), the transient
//! `last_scan_error`, and the branch-missing timer. The caller (the server
//! layer) appends under the chain lock. A failing scan returns no entries,
//! so it never partially reconciles. Keep refs (GC safety) are maintained
//! here as an idempotent side effect.
//!
//! - [`identity`] — `Change-Id:` trailer extraction and validation.
//! - [`objects`] — patch-ids and GC-safety keep refs.

pub mod identity;
pub mod objects;

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use git2::{BranchType, Commit, ErrorCode, Oid, Repository, Sort};

use crate::enums::LogKind;
use crate::review::{
    AddedRevision, ChainStatus, ChangeProj, LivePos, Projection, RevisionsPayload,
};

/// Documented scan error for chains containing merge commits.
pub const MERGE_COMMIT_ERROR: &str = "chain contains merge commits — rebase onto the base instead";

/// An entry the scan wants appended.
#[derive(Debug)]
pub struct NewEntry {
    pub kind: LogKind,
    pub payload: serde_json::Value,
}

/// The result of one scan. The caller appends `entries` (in order) and sets
/// the transient `error` / `branch_missing_since` on the projection.
#[derive(Debug)]
pub struct ScanResult {
    pub entries: Vec<NewEntry>,
    pub error: Option<String>,
    pub branch_missing_since: Option<String>,
}

impl ScanResult {
    fn nothing() -> ScanResult {
        ScanResult {
            entries: Vec::new(),
            error: None,
            branch_missing_since: None,
        }
    }

    fn failed(msg: String) -> ScanResult {
        ScanResult {
            entries: Vec::new(),
            error: Some(msg),
            branch_missing_since: None,
        }
    }

    fn closed(status: &'static str) -> ScanResult {
        ScanResult {
            entries: vec![NewEntry {
                kind: LogKind::ChainClosed,
                payload: serde_json::json!({ "status": status }),
            }],
            error: None,
            branch_missing_since: None,
        }
    }
}

/// Validate a registration: the repo opens and base/branch both resolve.
/// Only enforced when creating a *new* chain — an existing chain
/// re-registers even mid-rebase (the failure then surfaces as
/// `last_scan_error`).
///
/// # Errors
/// The 400 case of `POST /api/chains`.
pub fn validate_registration(git_dir: &std::path::Path, branch: &str, base: &str) -> Result<()> {
    let repo = Repository::open(git_dir).map_err(|e| {
        anyhow!(
            "cannot open repository {}: {}",
            git_dir.display(),
            e.message()
        )
    })?;
    repo.revparse_single(base)
        .and_then(|o| o.peel_to_commit())
        .map_err(|e| anyhow!("cannot resolve base '{base}': {}", e.message()))?;
    repo.find_branch(branch, BranchType::Local)
        .map_err(|e| anyhow!("cannot resolve branch '{branch}': {}", e.message()))?;
    Ok(())
}

/// Scan a chain against its projection. `alloc` mints fresh fold-assigned
/// ids for newly-seen changes. Never mutates the database; git ref keep
/// maintenance is the only side effect.
///
/// # Panics
/// When the chain length does not fit `u64` (a chain that long is
/// unreachable in practice).
#[expect(
    clippy::too_many_lines,
    reason = "the documented scan algorithm reads as one linear pass; \
              splitting its steps apart would obscure the contract"
)]
pub fn scan(proj: &Projection, now: jiff::Timestamp, alloc: &mut dyn FnMut() -> u64) -> ScanResult {
    let repo = match Repository::open(&proj.git_dir) {
        Ok(r) => r,
        Err(e) => {
            return ScanResult::failed(format!(
                "cannot open repository {}: {}",
                proj.git_dir,
                e.message()
            ));
        }
    };

    // Step 1: resolve base and tip.
    let base_commit = match repo
        .revparse_single(&proj.base)
        .and_then(|o| o.peel_to_commit())
    {
        Ok(c) => c,
        Err(e) => {
            return ScanResult::failed(format!(
                "cannot resolve base '{}': {}",
                proj.base,
                e.message()
            ));
        }
    };

    let tip = match repo.find_branch(&proj.branch, BranchType::Local) {
        Ok(branch) => match branch.get().peel_to_commit() {
            Ok(c) => c,
            Err(e) => {
                return ScanResult::failed(format!(
                    "cannot resolve branch '{}': {}",
                    proj.branch,
                    e.message()
                ));
            }
        },
        Err(e) if e.code() == ErrorCode::NotFound => return missing_branch(proj, now),
        Err(e) => {
            return ScanResult::failed(format!(
                "cannot resolve branch '{}': {}",
                proj.branch,
                e.message()
            ));
        }
    };

    // Step 2 (walk) happens early: closed chains only reopen when the branch
    // is alive *with commits*. Merge/root commits abort here.
    let commits = match walk_chain(&repo, base_commit.id(), tip.id()) {
        Ok(c) => c,
        Err(msg) => return ScanResult::failed(msg),
    };

    // Merged test: tip ancestor-or-equal of base (⇔ empty walk) plus the
    // patch-id quorum. tip == base *without* the quorum is just an empty
    // active chain.
    if proj.status == ChainStatus::Active && commits.is_empty() {
        let tip_in_base = tip.id() == base_commit.id()
            || repo
                .graph_descendant_of(base_commit.id(), tip.id())
                .unwrap_or(false);
        if tip_in_base && merged_quorum(&repo, proj, &base_commit) {
            objects::delete_chain_keep_refs(&repo, proj.chain_id);
            return ScanResult::closed("merged");
        }
    }

    // A closed (merged/abandoned) chain whose walk is empty stays closed: an
    // empty walk must not orphan its retained changes (it only reopens with
    // live commits, handled below).
    if commits.is_empty() && proj.status != ChainStatus::Active {
        return ScanResult::nothing();
    }

    let messages: Vec<String> = commits
        .iter()
        .map(|c| String::from_utf8_lossy(c.message_bytes()).into_owned())
        .collect();
    let short_shas: Vec<String> = commits
        .iter()
        .map(|c| c.id().to_string()[..12].to_string())
        .collect();

    // Step 2, identity validation.
    let keys = match identity::require_keys(&messages, &short_shas) {
        Ok(k) => k,
        Err(msg) => return ScanResult::failed(msg),
    };

    // Step 3: build the live set and the added revisions.
    let mut live = Vec::with_capacity(commits.len());
    let mut added = Vec::new();
    for (i, commit) in commits.iter().enumerate() {
        let key = &keys[i];
        let sha = commit.id().to_string();
        let position = u64::try_from(i).expect("chain length fits u64");
        let existing = proj.change_by_key(key);
        let change_id = existing.map_or_else(&mut *alloc, |c| c.id);
        live.push(LivePos {
            change_key: key.clone(),
            change_id,
            position,
        });

        let latest = existing.and_then(ChangeProj::latest_revision);
        if latest.is_some_and(|r| r.commit_sha == sha) {
            continue; // unchanged commit, no new revision
        }
        let parent_sha = match commit.parent_id(0) {
            Ok(o) => o.to_string(),
            Err(e) => return ScanResult::failed(format!("commit {sha} has no parent: {e}")),
        };
        let resets_status = match latest {
            Some(old) => !pure_rebase(&repo, &old.commit_sha, &old.message, &sha, &messages[i]),
            None => true,
        };
        added.push(AddedRevision {
            change_key: key.clone(),
            number: latest.map_or(1, |r| r.number + 1),
            commit_sha: sha,
            parent_sha,
            message: messages[i].clone(),
            resets_status,
        });
    }

    // A reopened (merged/abandoned) chain with live commits must re-emit even
    // when the structure is unchanged, so the fold flips it back to active.
    let reopen = proj.status != ChainStatus::Active && !commits.is_empty();
    let new_live: Vec<(&str, u64)> = live
        .iter()
        .map(|l| (l.change_key.as_str(), l.position))
        .collect();
    let current_live: Vec<(&str, u64)> = proj
        .changes_ordered()
        .iter()
        .filter(|c| !c.orphaned)
        .map(|c| (c.change_key.as_str(), c.position.unwrap_or(0)))
        .collect();
    if added.is_empty() && new_live == current_live && !reopen {
        return ScanResult::nothing(); // no structural change; clears any prior error
    }

    maintain_keep_refs(&repo, proj, &live, &added);

    ScanResult {
        entries: vec![NewEntry {
            kind: LogKind::Revisions,
            payload: serde_json::to_value(RevisionsPayload { live, added })
                .unwrap_or_else(|_| serde_json::json!({})),
        }],
        error: None,
        branch_missing_since: None,
    }
}

/// Keep refs for every revision (live and orphan) so history stays
/// renderable — idempotent (docs/data-model.md "GC safety").
fn maintain_keep_refs(
    repo: &Repository,
    proj: &Projection,
    live: &[LivePos],
    added: &[AddedRevision],
) {
    let id_of: HashMap<&str, u64> = live
        .iter()
        .map(|l| (l.change_key.as_str(), l.change_id))
        .collect();
    for change in &proj.changes {
        for rev in &change.revisions {
            objects::ensure_keep_ref(repo, proj.chain_id, change.id, rev.number, &rev.commit_sha);
        }
    }
    // Every added revision's key is also in `live` (scan pushes both
    // together), so id_of is exhaustive over added keys.
    for a in added {
        if let Some(&change_id) = id_of.get(a.change_key.as_str()) {
            objects::ensure_keep_ref(repo, proj.chain_id, change_id, a.number, &a.commit_sha);
        }
    }
}

/// The branch ref is gone. Closed chains stay closed quietly; an active
/// chain is only abandoned after the ref is missing on two consecutive
/// scans ≥ 10s apart (mid-rebase protection).
fn missing_branch(proj: &Projection, now: jiff::Timestamp) -> ScanResult {
    if proj.status != ChainStatus::Active {
        return ScanResult::nothing();
    }
    let marker = format!("branch '{}' not found", proj.branch);
    match proj
        .branch_missing_since
        .as_deref()
        .and_then(|s| s.parse::<jiff::Timestamp>().ok())
    {
        Some(prev) if now.as_second() - prev.as_second() >= 10 => {
            let mut res = ScanResult::closed("abandoned");
            // delete_chain_keep_refs needs the repo; the branch is gone but
            // the repo opens — best effort via the caller's next scan.
            if let Ok(repo) = Repository::open(&proj.git_dir) {
                objects::delete_chain_keep_refs(&repo, proj.chain_id);
            }
            res.branch_missing_since = None;
            res
        }
        Some(prev) => ScanResult {
            entries: Vec::new(),
            error: Some(marker),
            branch_missing_since: Some(prev.to_string()),
        },
        None => ScanResult {
            entries: Vec::new(),
            error: Some(marker),
            branch_missing_since: Some(now.to_string()),
        },
    }
}

/// Walk `base..tip` oldest-first. Any merge commit aborts the scan with the
/// documented error; so does a root commit (the diff/identity model needs a
/// first parent everywhere). Returns the abort message on `Err`.
fn walk_chain(repo: &Repository, base: Oid, tip: Oid) -> Result<Vec<Commit<'_>>, String> {
    let mut walk = repo.revwalk().map_err(|e| e.to_string())?;
    walk.push(tip).map_err(|e| e.to_string())?;
    walk.hide(base).map_err(|e| e.to_string())?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)
        .map_err(|e| e.to_string())?;
    let mut commits = Vec::new();
    for oid in walk {
        let oid = oid.map_err(|e| e.to_string())?;
        let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
        match commit.parent_count() {
            0 => {
                return Err(
                    "chain contains a root commit — the base must be an ancestor of the branch"
                        .to_string(),
                );
            }
            1 => {}
            _ => return Err(MERGE_COMMIT_ERROR.to_string()),
        }
        commits.push(commit);
    }
    Ok(commits)
}

/// Merged quorum: every live non-empty change's patch-id (or Change-Id
/// trailer) must appear in `fork..base`, where fork is the chain's recorded
/// fork point (the first live change's latest revision's parent). At least
/// one real match required. Anything unverifiable counts against merging.
fn merged_quorum(repo: &Repository, proj: &Projection, base: &Commit) -> bool {
    // Candidates: live changes' latest revisions; if none (an earlier failed
    // quorum orphaned everything), fall back to the orphans.
    let mut candidates: Vec<(&str, &str, &str)> = Vec::new(); // (key, commit_sha, parent_sha)
    for change in proj.changes_ordered() {
        if change.orphaned {
            continue;
        }
        match change.latest_revision() {
            Some(rev) => candidates.push((&change.change_key, &rev.commit_sha, &rev.parent_sha)),
            None => return false,
        }
    }
    if candidates.is_empty() {
        for change in &proj.changes {
            if let Some(rev) = change.latest_revision() {
                candidates.push((&change.change_key, &rev.commit_sha, &rev.parent_sha));
            }
        }
    }
    let Some((_, _, fork_sha)) = candidates.first() else {
        return false;
    };
    let Ok(fork) = Oid::from_str(fork_sha) else {
        return false;
    };

    let mut base_patch_ids: HashSet<String> = HashSet::new();
    let mut base_trailers: HashSet<String> = HashSet::new();
    let Ok(mut walk) = repo.revwalk() else {
        return false;
    };
    if walk.push(base.id()).is_err() || walk.hide(fork).is_err() {
        return false;
    }
    for oid in walk {
        let Ok(oid) = oid else { return false };
        let Ok(commit) = repo.find_commit(oid) else {
            return false;
        };
        if let Some(trailer) =
            identity::change_id_trailer(&String::from_utf8_lossy(commit.message_bytes()))
        {
            base_trailers.insert(trailer);
        }
        if commit.parent_count() == 1
            && let (Ok(parent_tree), Ok(tree)) =
                (commit.parent(0).and_then(|p| p.tree()), commit.tree())
            && let Ok(pid) = objects::tree_patch_id(repo, &parent_tree, &tree)
        {
            base_patch_ids.insert(pid);
        }
    }

    let mut any_matched = false;
    for (key, commit_sha, _) in &candidates {
        if base_trailers.contains(*key) {
            any_matched = true;
            continue;
        }
        match objects::sha_patch_id(repo, commit_sha) {
            Some(pid) if pid == objects::EMPTY_PATCH_ID => {}
            Some(pid) if base_patch_ids.contains(&pid) => any_matched = true,
            _ => return false, // unverifiable or unmatched
        }
    }
    any_matched
}

/// True when a revision differs from the previous one only by a rebase: a
/// patch-id-equal commit with an unchanged message (the predicate behind
/// review auto-retargeting in api.md). Unverifiable objects make it false.
#[must_use]
pub fn pure_rebase(
    repo: &Repository,
    old_sha: &str,
    old_msg: &str,
    new_sha: &str,
    new_msg: &str,
) -> bool {
    if old_msg != new_msg {
        return false;
    }
    old_sha == new_sha
        || matches!(
            (objects::sha_patch_id(repo, old_sha), objects::sha_patch_id(repo, new_sha)),
            (Some(x), Some(y)) if x == y
        )
}
