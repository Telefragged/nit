//! Git scan engine: reconciles a registered branch (`base..tip`) with the
//! review database — docs/data-model.md "Scan algorithm" is the contract.
//!
//! [`scan`] runs the whole algorithm in one `BEGIN IMMEDIATE` transaction
//! under a caller-provided exclusive context (the per-chain lock lives in
//! the server layer). A failing scan never partially reconciles: the
//! transaction rolls back and the failure is recorded in
//! `chains.last_scan_error` while previous state stays served.
//!
//! - [`identity`] — `Change-Id:` trailer extraction, the
//!   required-Change-Id validation, and commit subject extraction.
//! - [`objects`] — patch-ids and GC-safety keep refs.

pub mod identity;
pub mod objects;

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use git2::{BranchType, Commit, ErrorCode, Oid, Repository, Sort};
use rusqlite::{Connection, TransactionBehavior};

use crate::db::{self, ChainStatus, ChangeStatus};

/// Documented scan error for chains containing merge commits.
pub const MERGE_COMMIT_ERROR: &str = "chain contains merge commits — rebase onto the base instead";

/// What one scan did.
#[derive(Debug)]
pub struct ScanOutcome {
    /// The chain row after the scan (post-reconcile or with
    /// `last_scan_error` set).
    pub chain: db::Chain,
    /// Whether the scan made a net structural difference (and emitted a
    /// `chain_updated`/`chain_closed` event).
    pub updated: bool,
}

/// A scan-level failure: rolls the transaction back and is recorded in
/// `chains.last_scan_error`, never propagated as a hard error.
#[derive(Debug, thiserror::Error)]
enum ScanAbort {
    #[error("{0}")]
    Failed(String),
    /// The branch ref is missing — recorded like a failure, but the
    /// timestamp of its first observation drives the two-scan abandoned
    /// rule, so `updated_at` must not be re-bumped on repeats.
    #[error("{0}")]
    BranchMissing(String),
}

fn fail(msg: String) -> anyhow::Error {
    anyhow::Error::new(ScanAbort::Failed(msg))
}

/// Register (or refresh) a chain: canonicalize the repo path, auto-create
/// the repo row, upsert the chain (idempotent; re-registration updates
/// `base`). Does not scan.
///
/// # Errors
/// When the repo can't be opened or branch/base don't resolve — the 400
/// case of `POST /api/chains`.
pub fn register(
    conn: &Connection,
    repo_path: &std::path::Path,
    branch: &str,
    base: &str,
) -> Result<db::Chain> {
    let canonical = std::fs::canonicalize(repo_path)
        .map_err(|e| anyhow!("cannot resolve repo path {}: {e}", repo_path.display()))?;
    let repo = Repository::open(&canonical).map_err(|e| {
        anyhow!(
            "cannot open repository {}: {}",
            canonical.display(),
            e.message()
        )
    })?;
    repo.revparse_single(base)
        .and_then(|o| o.peel_to_commit())
        .map_err(|e| anyhow!("cannot resolve base '{base}': {}", e.message()))?;
    repo.find_branch(branch, BranchType::Local)
        .map_err(|e| anyhow!("cannot resolve branch '{branch}': {}", e.message()))?;
    let canonical = canonical
        .to_str()
        .ok_or_else(|| anyhow!("repo path is not valid UTF-8"))?;
    let repo_row = db::get_or_create_repo(conn, canonical)?;
    db::get_or_create_chain(conn, repo_row.id, branch, base)
}

/// Scan a chain: walk `base..tip` and reconcile the database. The caller
/// must hold the chain's exclusive lock.
///
/// # Errors
/// Only infrastructure problems (unknown chain, broken database) —
/// git-level failures are recorded in `last_scan_error` on the returned
/// chain instead.
pub fn scan(conn: &mut Connection, chain_id: i64) -> Result<ScanOutcome> {
    scan_at(conn, chain_id, jiff::Timestamp::now())
}

/// [`scan`] with an injectable clock — the abandoned-branch rule compares
/// `now` against the previous scan's timestamp. Tests use this; everyone
/// else wants [`scan`].
///
/// # Errors
/// See [`scan`].
pub fn scan_at(conn: &mut Connection, chain_id: i64, now: jiff::Timestamp) -> Result<ScanOutcome> {
    let chain =
        db::get_chain(conn, chain_id)?.ok_or_else(|| anyhow!("chain {chain_id} not found"))?;
    let repo_path = db::chain_repo_path(conn, chain_id)?
        .ok_or_else(|| anyhow!("chain {chain_id}: repo row missing"))?;
    let now_str = now.to_string();

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    match reconcile(&tx, &repo_path, &chain, now) {
        Ok(updated) => {
            if chain.last_scan_error.is_some() {
                db::chain_set_scan_error(&tx, chain_id, None, &now_str, true)?;
            }
            tx.commit()?;
            let chain = db::get_chain(conn, chain_id)?
                .ok_or_else(|| anyhow!("chain {chain_id} vanished"))?;
            Ok(ScanOutcome { chain, updated })
        }
        Err(err) => {
            drop(tx); // rollback — a failed scan never partially reconciles
            if err.downcast_ref::<rusqlite::Error>().is_some() {
                return Err(err); // the db itself is broken; don't try to record
            }
            let (msg, touch) = match err.downcast_ref::<ScanAbort>() {
                Some(ScanAbort::BranchMissing(m)) => {
                    // Keep updated_at of the scan that first saw it missing.
                    let repeat = chain.last_scan_error.as_deref() == Some(m.as_str());
                    (m.clone(), !repeat)
                }
                Some(ScanAbort::Failed(m)) => (m.clone(), true),
                None => (format!("scan failed: {err:#}"), true),
            };
            db::chain_set_scan_error(conn, chain_id, Some(&msg), &now_str, touch)?;
            let chain = db::get_chain(conn, chain_id)?
                .ok_or_else(|| anyhow!("chain {chain_id} vanished"))?;
            Ok(ScanOutcome {
                chain,
                updated: false,
            })
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "transaction script for the documented scan algorithm; \
              splitting the steps apart would obscure the contract"
)]
fn reconcile(
    tx: &Connection,
    repo_path: &str,
    chain: &db::Chain,
    now: jiff::Timestamp,
) -> Result<bool> {
    let now_str = now.to_string();
    let repo = Repository::open(repo_path).map_err(|e| {
        fail(format!(
            "cannot open repository {repo_path}: {}",
            e.message()
        ))
    })?;

    // Step 1: resolve base and tip.
    let base_commit = repo
        .revparse_single(&chain.base)
        .and_then(|o| o.peel_to_commit())
        .map_err(|e| {
            fail(format!(
                "cannot resolve base '{}': {}",
                chain.base,
                e.message()
            ))
        })?;

    let tip = match repo.find_branch(&chain.branch, BranchType::Local) {
        Ok(branch) => branch.get().peel_to_commit().map_err(|e| {
            fail(format!(
                "cannot resolve branch '{}': {}",
                chain.branch,
                e.message()
            ))
        })?,
        Err(e) if e.code() == ErrorCode::NotFound => {
            return missing_branch(tx, &repo, chain, now);
        }
        Err(e) => {
            return Err(fail(format!(
                "cannot resolve branch '{}': {}",
                chain.branch,
                e.message()
            )));
        }
    };

    // Step 2 (walk) happens early because closed chains only reopen when
    // the branch is alive *with commits*. Merge/root commits abort here.
    let commits = walk_chain(&repo, base_commit.id(), tip.id())?;

    if chain.status != ChainStatus::Active {
        if commits.is_empty() {
            return Ok(false);
        }
        db::chain_set_status(tx, chain.id, ChainStatus::Active, &now_str)?;
    }
    let mut updated = chain.status != ChainStatus::Active; // reopened

    // Step 1, merged test: tip ancestor-or-equal of base (⇔ empty walk)
    // plus the patch-id quorum. tip == base *without* the quorum is just an
    // empty active chain.
    if chain.status == ChainStatus::Active && commits.is_empty() {
        let tip_in_base = tip.id() == base_commit.id()
            || repo
                .graph_descendant_of(base_commit.id(), tip.id())
                .unwrap_or(false);
        if tip_in_base && merged_quorum(tx, &repo, chain, &base_commit)? {
            close_chain(tx, &repo, chain, ChainStatus::Merged, &now_str)?;
            return Ok(true);
        }
    }

    let messages: Vec<String> = commits
        .iter()
        .map(|c| String::from_utf8_lossy(c.message_bytes()).into_owned())
        .collect();
    let shas: Vec<String> = commits.iter().map(|c| c.id().to_string()).collect();

    // Step 2, identity validation: every commit carries its own Change-Id
    // trailer (and is not a fixup!/squash! commit) or the scan aborts.
    let short_shas: Vec<String> = shas.iter().map(|s| s[..12].to_string()).collect();
    let keys = identity::require_keys(&messages, &short_shas).map_err(fail)?;

    // Step 3: match commits to change rows — the Change-Id key is the
    // identity (docs/data-model.md "Change identity").
    let existing = db::changes_for_chain(tx, chain.id)?; // live by position, orphans last
    let row_by_key: HashMap<&str, usize> = existing
        .iter()
        .enumerate()
        .map(|(ei, row)| (row.change_key.as_str(), ei))
        .collect();
    let matched: Vec<Option<usize>> = keys
        .iter()
        .map(|key| row_by_key.get(key.as_str()).copied())
        .collect();

    // Step 4: per live change, insert a new revision when the commit moved.
    let claimed: HashSet<usize> = matched.iter().flatten().copied().collect();
    for (i, commit) in commits.iter().enumerate() {
        let sha = shas[i].as_str();
        let position = i64::try_from(i).expect("chain length fits i64");

        let (change_id, stored_position, stored_status, mut status, is_new) = if let Some(ei) =
            matched[i]
        {
            let row = &existing[ei];
            let status = if row.status == ChangeStatus::Orphaned {
                // Re-attachment: status returns to its pre-orphan value,
                // re-derived from the review history.
                pre_orphan_status(tx, &repo, row.id)?
            } else {
                row.status
            };
            (row.id, row.position, row.status, status, false)
        } else {
            let row = db::insert_change(tx, chain.id, &keys[i], position, ChangeStatus::Pending)?;
            updated = true;
            (
                row.id,
                Some(position),
                ChangeStatus::Pending,
                ChangeStatus::Pending,
                true,
            )
        };

        let latest = db::latest_revision(tx, change_id)?;
        let same_state = latest.as_ref().is_some_and(|l| l.commit_sha == sha);
        if !same_state {
            let number = latest.as_ref().map_or(1, |l| l.number + 1);
            let parent_sha = commit.parent_id(0)?.to_string();
            let new_rev = db::insert_revision(
                tx,
                change_id,
                number,
                sha,
                &parent_sha,
                &messages[i],
                &now_str,
            )?;
            updated = true;
            // Rule 4 status effect: pure rebase keeps status; anything
            // else means the reviewer must look again.
            let pure = latest
                .as_ref()
                .is_some_and(|l| pure_rebase_equivalent(&repo, l, &new_rev));
            if !pure {
                status = ChangeStatus::Pending;
            }
        }

        if !is_new && (stored_position != Some(position) || stored_status != status) {
            db::change_set_position_status(tx, change_id, Some(position), status)?;
            updated = true;
        }
    }

    // Step 3, orphaning: live rows whose commit matched nothing. Never
    // deleted — comments, drafts and reviews stay.
    for (ei, row) in existing.iter().enumerate() {
        if !claimed.contains(&ei) && row.status != ChangeStatus::Orphaned {
            db::change_set_position_status(tx, row.id, None, ChangeStatus::Orphaned)?;
            updated = true;
        }
    }

    // GC safety: keep refs for every revision.
    for row in db::changes_for_chain(tx, chain.id)? {
        for rev in db::revisions_for_change(tx, row.id)? {
            objects::ensure_keep_ref(&repo, chain.id, row.id, &rev);
        }
    }

    // Step 5: net structural difference → one chain_updated event.
    if updated {
        db::insert_event(
            tx,
            chain.id,
            "chain_updated",
            &serde_json::json!({"chain_id": chain.id}),
            &now_str,
        )?;
        db::chain_touch(tx, chain.id, &now_str)?;
    }
    Ok(updated)
}

/// Walk `base..tip` oldest-first. Any merge commit aborts the scan with
/// the documented error; so does a root commit (the diff/identity model
/// needs a first parent everywhere).
fn walk_chain(repo: &Repository, base: Oid, tip: Oid) -> Result<Vec<Commit<'_>>> {
    let mut walk = repo.revwalk()?;
    walk.push(tip)?;
    walk.hide(base)?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
    let mut commits = Vec::new();
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        match commit.parent_count() {
            0 => {
                return Err(fail(
                    "chain contains a root commit — the base must be an ancestor of the branch"
                        .to_string(),
                ));
            }
            1 => {}
            _ => return Err(fail(MERGE_COMMIT_ERROR.to_string())),
        }
        commits.push(commit);
    }
    Ok(commits)
}

/// The branch ref is gone. Closed chains stay closed quietly; an active
/// chain is only abandoned after the ref is missing on two consecutive
/// scans ≥ 10s apart (mid-rebase protection) — otherwise the missing ref
/// is recorded like a scan failure.
fn missing_branch(
    tx: &Connection,
    repo: &Repository,
    chain: &db::Chain,
    now: jiff::Timestamp,
) -> Result<bool> {
    if chain.status != ChainStatus::Active {
        return Ok(false);
    }
    let marker = format!("branch '{}' not found", chain.branch);
    if chain.last_scan_error.as_deref() == Some(marker.as_str())
        && let Ok(prev) = chain.updated_at.parse::<jiff::Timestamp>()
        && now.as_second() - prev.as_second() >= 10
    {
        close_chain(tx, repo, chain, ChainStatus::Abandoned, &now.to_string())?;
        return Ok(true);
    }
    Err(anyhow::Error::new(ScanAbort::BranchMissing(marker)))
}

fn close_chain(
    tx: &Connection,
    repo: &Repository,
    chain: &db::Chain,
    status: ChainStatus,
    now_str: &str,
) -> Result<()> {
    db::chain_set_status(tx, chain.id, status, now_str)?;
    db::chain_set_scan_error(tx, chain.id, None, now_str, false)?;
    db::insert_event(
        tx,
        chain.id,
        "chain_closed",
        &serde_json::json!({"chain_id": chain.id, "status": status.as_str()}),
        now_str,
    )?;
    objects::delete_chain_keep_refs(repo, chain.id);
    Ok(())
}

/// Merged quorum (step 1): every live non-empty change's patch-id must
/// appear in `fork..base`, where fork is the chain's recorded fork point
/// (the first live change's parent). At least one non-empty live change
/// must vote, else `tip == base` is just an empty active chain. Anything
/// unverifiable counts against merging.
fn merged_quorum(
    tx: &Connection,
    repo: &Repository,
    chain: &db::Chain,
    base: &Commit,
) -> Result<bool> {
    let rows = db::changes_for_chain(tx, chain.id)?;
    let mut candidates: Vec<(String, db::Revision)> = Vec::new();
    for row in &rows {
        if row.position.is_none() {
            continue;
        }
        match db::latest_revision(tx, row.id)? {
            Some(rev) => candidates.push((row.change_key.clone(), rev)),
            None => return Ok(false),
        }
    }
    // A quorum that failed on the first post-merge scan (e.g. patch-id
    // context drift, see below) orphans every change; later scans must
    // still be able to recognize the merge from the orphans. Safe against
    // reset-to-base rebuilds: those match neither trailers nor patch-ids.
    if candidates.is_empty() {
        for row in &rows {
            if let Some(rev) = db::latest_revision(tx, row.id)? {
                candidates.push((row.change_key.clone(), rev));
            }
        }
    }
    let Some((_, first)) = candidates.first() else {
        return Ok(false);
    };
    let Ok(fork) = Oid::from_str(&first.parent_sha) else {
        return Ok(false);
    };

    // Patch-ids and Change-Id trailers of base-side commits in fork..base
    // (first-parent diffs; merge commits don't carry a meaningful single
    // patch-id, but their trailers still count).
    let mut base_patch_ids: HashSet<String> = HashSet::new();
    let mut base_trailers: HashSet<String> = HashSet::new();
    let Ok(mut walk) = repo.revwalk() else {
        return Ok(false);
    };
    if walk.push(base.id()).is_err() || walk.hide(fork).is_err() {
        return Ok(false);
    }
    for oid in walk {
        let Ok(oid) = oid else { return Ok(false) };
        let Ok(commit) = repo.find_commit(oid) else {
            return Ok(false);
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

    // A change matches by Change-Id trailer first (immune to the patch-id
    // context drift that rewriting a *neighboring* change causes), then by
    // patch-id. Empty diffs are trivially matched but don't count toward
    // the quorum.
    let mut any_matched = false;
    for (key, rev) in &candidates {
        if base_trailers.contains(key) {
            any_matched = true;
            continue;
        }
        match objects::sha_patch_id(repo, &rev.commit_sha) {
            Some(pid) if pid == objects::EMPTY_PATCH_ID => {}
            Some(pid) if base_patch_ids.contains(&pid) => any_matched = true,
            // Unverifiable (pruned objects) or unmatched.
            _ => return Ok(false),
        }
    }
    Ok(any_matched)
}

/// True when two revisions differ only by a rebase: a patch-id-equal
/// commit with an unchanged commit message (rule 4; the same predicate is
/// behind review auto-retargeting in api.md). Messages compare for exact
/// equality — they are reviewable as `/COMMIT_MSG`, so a reword must put
/// the change back in front of the reviewer; true rebases replay them
/// verbatim. Unverifiable objects make it false — the reviewer looks
/// again.
#[must_use]
pub fn pure_rebase_equivalent(repo: &Repository, old: &db::Revision, new: &db::Revision) -> bool {
    if old.message != new.message {
        return false;
    }
    old.commit_sha == new.commit_sha
        || matches!(
            (
                objects::sha_patch_id(repo, &old.commit_sha),
                objects::sha_patch_id(repo, &new.commit_sha),
            ),
            (Some(x), Some(y)) if x == y
        )
}

/// A change's pre-orphan status, re-derived from review history — exactly
/// what the status machine would have produced: a review counts if it sits
/// on the latest revision *or* on any older revision connected to it by an
/// unbroken run of pure rebases (rule 4 keeps status across those, and the
/// review row stays on the old revision number).
fn pre_orphan_status(tx: &Connection, repo: &Repository, change_id: i64) -> Result<ChangeStatus> {
    let revisions = db::revisions_for_change(tx, change_id)?; // ascending
    let Some(latest) = revisions.last() else {
        return Ok(ChangeStatus::Pending);
    };
    let mut eligible = vec![latest.number];
    let mut newer = latest;
    for older in revisions.iter().rev().skip(1) {
        if !pure_rebase_equivalent(repo, older, newer) {
            break;
        }
        eligible.push(older.number);
        newer = older;
    }
    let mut best: Option<db::Review> = None;
    for number in eligible {
        if let Some(review) = db::latest_review_on_revision(tx, change_id, number)?
            && best.as_ref().is_none_or(|b| review.id > b.id)
        {
            best = Some(review);
        }
    }
    Ok(match best {
        Some(review) => match review.verdict.as_str() {
            "approve" => ChangeStatus::Approved,
            "request_changes" => ChangeStatus::ChangesRequested,
            "comment" => ChangeStatus::Commented,
            _ => ChangeStatus::Pending,
        },
        None => ChangeStatus::Pending,
    })
}
