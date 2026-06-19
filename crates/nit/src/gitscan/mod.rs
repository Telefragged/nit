//! The git layer: the push walk, merged/abandoned detection for the
//! background timer, and tip-name resolution — docs/data-model.md
//! ("Push", "Lifecycle") is the contract.
//!
//! Everything here is pure with respect to the database: it reads git and
//! returns values the caller (the api layer) folds into the per-change logs.
//! Keep refs (GC safety) are an idempotent side effect.
//!
//! - [`identity`] — `Change-Id:` trailer extraction and validation.
//! - [`objects`] — patch-ids and GC-safety keep refs.

pub mod identity;
pub mod objects;

use std::collections::HashSet;

use git2::{BranchType, Commit, Oid, Repository, Sort};

use crate::review::ChangeProj;

/// Documented push error for chains containing merge commits.
pub const MERGE_COMMIT_ERROR: &str = "chain contains merge commits — rebase onto the base instead";

/// A commit sha truncated to 12 chars — the canonical short form for display.
#[must_use]
pub fn short_sha(sha: &str) -> String {
    sha.chars().take(12).collect()
}

/// One commit the push walk recorded, oldest-first. `parent_sha` is its first
/// parent (the previous member, or the fork for the first); `base_sha` is the
/// whole walk's fork point on the canonical branch.
#[derive(Debug, Clone)]
pub struct WalkedCommit {
    pub change_key: String,
    pub commit_sha: String,
    pub parent_sha: String,
    pub message: String,
}

/// The result of a push walk: the fork point on the canonical branch and the
/// commits between it and the tip, oldest-first.
#[derive(Debug, Clone)]
pub struct PushWalk {
    pub fork_sha: String,
    pub commits: Vec<WalkedCommit>,
}

/// Resolve a refish to a commit oid, with a human message on failure.
fn resolve_commit(repo: &Repository, refish: &str) -> Result<Oid, String> {
    repo.revparse_single(refish)
        .and_then(|o| o.peel_to_commit())
        .map(|c| c.id())
        .map_err(|e| format!("cannot resolve '{refish}': {}", e.message()))
}

/// Walk `merge-base(base, tip)..tip` oldest-first and validate it
/// (docs/data-model.md "Push"). The whole walk is all-or-nothing: any
/// structural fault is an `Err(message)` the caller maps to a 400.
///
/// # Errors
/// When the repo/base/tip can't be resolved, there is no merge base, or the
/// walk contains a merge/root commit, a missing/duplicate `Change-Id`, or a
/// `fixup!`/`squash!` subject.
pub fn walk_push(git_dir: &str, base: &str, tip: &str) -> Result<PushWalk, String> {
    let repo = Repository::open(git_dir)
        .map_err(|e| format!("cannot open repository {git_dir}: {}", e.message()))?;
    let base_oid = resolve_commit(&repo, base)?;
    let tip_oid = resolve_commit(&repo, tip)?;
    let fork = repo.merge_base(base_oid, tip_oid).map_err(|e| {
        format!(
            "no merge base between '{base}' and '{tip}': {}",
            e.message()
        )
    })?;

    let commits = walk_linear(&repo, fork, tip_oid)?;
    let messages: Vec<String> = commits
        .iter()
        .map(|c| String::from_utf8_lossy(c.message_bytes()).into_owned())
        .collect();
    let short_shas: Vec<String> = commits
        .iter()
        .map(|c| short_sha(&c.id().to_string()))
        .collect();
    let keys = identity::require_keys(&messages, &short_shas)?;

    let mut walked = Vec::with_capacity(commits.len());
    let mut prev = fork.to_string();
    for (i, commit) in commits.iter().enumerate() {
        let sha = commit.id().to_string();
        walked.push(WalkedCommit {
            change_key: keys[i].clone(),
            commit_sha: sha.clone(),
            parent_sha: prev.clone(),
            message: messages[i].clone(),
        });
        prev = sha;
    }
    Ok(PushWalk {
        fork_sha: fork.to_string(),
        commits: walked,
    })
}

/// Walk `base..tip` oldest-first, rejecting merge and root commits (the
/// diff/identity model needs a single first parent everywhere).
fn walk_linear(repo: &Repository, base: Oid, tip: Oid) -> Result<Vec<Commit<'_>>, String> {
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

/// True when a revision differs from the previous one only by a rebase: a
/// patch-id-equal commit with an unchanged message. Unverifiable objects make
/// it false.
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

/// Whether the change's latest revision has **landed** on the canonical
/// branch, returning the landed revision number (docs/data-model.md "The
/// per-change merge timer"). The window is `base_sha..canonical`:
///
/// 1. **Change-Id match** — a commit in the window carries this change's key.
///    If its patch-id matches the latest revision's, it landed; if it differs
///    ("previously landed, now amended"), the change stays open (None).
/// 2. **Patch-id match** — else the latest revision's patch-id appears in the
///    window. An empty diff never alone counts as a landing.
#[must_use]
pub fn landed_revision(repo: &Repository, base_branch: &str, change: &ChangeProj) -> Option<u64> {
    let latest = change.latest_revision()?;
    let fork = Oid::from_str(&latest.base_sha).ok()?;
    let base = resolve_commit(repo, base_branch).ok()?;

    let mut base_pids: HashSet<String> = HashSet::new();
    let mut keyed_pid: Option<Option<String>> = None; // Some(pid?) if a commit carries this key
    let mut walk = repo.revwalk().ok()?;
    walk.push(base).ok()?;
    walk.hide(fork).ok()?;
    for oid in walk {
        let Ok(oid) = oid else { return None };
        let Ok(commit) = repo.find_commit(oid) else {
            return None;
        };
        let pid = (commit.parent_count() == 1)
            .then(|| objects::sha_patch_id(repo, &oid.to_string()))
            .flatten();
        if let Some(p) = &pid {
            base_pids.insert(p.clone());
        }
        if let Some(trailer) =
            identity::change_id_trailer(&String::from_utf8_lossy(commit.message_bytes()))
            && trailer == change.change_key
        {
            keyed_pid = Some(pid);
        }
    }

    let latest_pid = objects::sha_patch_id(repo, &latest.commit_sha);
    if let Some(landed) = keyed_pid {
        // The Change-Id is in the base. Landed only if the patch-id matches the
        // current revision; a mismatch is "landed earlier, since amended".
        return match (landed, &latest_pid) {
            (Some(a), Some(b)) if a == *b && a != objects::EMPTY_PATCH_ID => Some(latest.number),
            _ => None,
        };
    }
    match latest_pid {
        Some(pid) if pid != objects::EMPTY_PATCH_ID && base_pids.contains(&pid) => {
            Some(latest.number)
        }
        _ => None,
    }
}

/// Best-effort display name for a tip commit (docs/data-model.md "Tips"): a
/// local branch pointing exactly at it, else one that contains it, else
/// `None` (the caller falls back to the commit subject). nit stores no branch
/// key — names are resolved here at query time.
#[must_use]
pub fn tip_name(repo: &Repository, tip_sha: &str) -> Option<String> {
    let oid = Oid::from_str(tip_sha).ok()?;
    let branches = repo.branches(Some(BranchType::Local)).ok()?;
    let mut contains: Option<String> = None;
    for branch in branches.flatten() {
        let Some(name) = branch.0.name().ok().flatten().map(str::to_string) else {
            continue;
        };
        let Ok(target) = branch.0.get().peel_to_commit().map(|c| c.id()) else {
            continue;
        };
        if target == oid {
            return Some(name);
        }
        if contains.is_none() && repo.graph_descendant_of(target, oid).unwrap_or(false) {
            contains = Some(name);
        }
    }
    contains
}

/// The keep-ref maintenance for one change's revisions — idempotent
/// (docs/data-model.md "Keep refs").
pub fn maintain_keep_refs(repo: &Repository, change: &ChangeProj) {
    for rev in &change.revisions {
        objects::ensure_keep_ref(repo, change.id, rev.number, &rev.commit_sha);
    }
}
