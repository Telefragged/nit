//! Git scan engine: reconciles a registered branch (`base..tip`) with the
//! review database — docs/data-model.md "Scan algorithm" is the contract.
//!
//! [`scan`] runs the whole algorithm in one `BEGIN IMMEDIATE` transaction
//! under a caller-provided exclusive context (the per-chain lock lives in
//! the server layer). A failing scan never partially reconciles: the
//! transaction rolls back and the failure is recorded in
//! `chains.last_scan_error` while previous state stays served.
//!
//! - [`fixup`] — `fixup!`/`squash!` classification and autosquash target
//!   attachment (pure logic, differentially tested against git).
//! - [`identity`] — `Change-Id:` trailer extraction and duplicate-trailer
//!   derived keys.
//! - [`fold`] — patch-ids, effective-tree folding, GC-safety keep refs.

pub mod fixup;
pub mod fold;
pub mod identity;

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use git2::{BranchType, Commit, ErrorCode, Oid, Repository, Sort};
use rusqlite::{Connection, TransactionBehavior};

use crate::db::{self, ChainStatus, ChangeStatus};
use fixup::CommitMeta;

/// Documented scan error for chains containing merge commits.
pub const MERGE_COMMIT_ERROR: &str = "chain contains merge commits — rebase onto the base instead";

/// What one scan did. `warnings` (duplicate Change-Id, squash! commits)
/// are per-scan and surface in the push response / chain banner; they are
/// not persisted.
#[derive(Debug)]
pub struct ScanOutcome {
    /// The chain row after the scan (post-reconcile or with
    /// `last_scan_error` set).
    pub chain: db::Chain,
    pub warnings: Vec<String>,
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

struct Recon {
    updated: bool,
    warnings: Vec<String>,
}

/// Register (or refresh) a chain: canonicalize the repo path, auto-create
/// the repo row, upsert the chain (idempotent; re-registration updates
/// `base`). Errors when the repo can't be opened or branch/base don't
/// resolve — the 400 case of `POST /api/chains`. Does not scan.
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
/// must hold the chain's exclusive lock. Returns `Err` only for
/// infrastructure problems (unknown chain, broken database); git-level
/// failures are recorded in `last_scan_error` on the returned chain.
pub fn scan(conn: &mut Connection, chain_id: i64) -> Result<ScanOutcome> {
    scan_at(conn, chain_id, jiff::Timestamp::now())
}

/// [`scan`] with an injectable clock — the abandoned-branch rule compares
/// `now` against the previous scan's timestamp. Tests use this; everyone
/// else wants [`scan`].
pub fn scan_at(conn: &mut Connection, chain_id: i64, now: jiff::Timestamp) -> Result<ScanOutcome> {
    let chain =
        db::get_chain(conn, chain_id)?.ok_or_else(|| anyhow!("chain {chain_id} not found"))?;
    let repo_path = db::chain_repo_path(conn, chain_id)?
        .ok_or_else(|| anyhow!("chain {chain_id}: repo row missing"))?;
    let now_str = now.to_string();

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    match reconcile(&tx, &repo_path, &chain, now) {
        Ok(rec) => {
            if chain.last_scan_error.is_some() {
                db::chain_set_scan_error(&tx, chain_id, None, &now_str, true)?;
            }
            tx.commit()?;
            let chain = db::get_chain(conn, chain_id)?
                .ok_or_else(|| anyhow!("chain {chain_id} vanished"))?;
            Ok(ScanOutcome {
                chain,
                warnings: rec.warnings,
                updated: rec.updated,
            })
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
                warnings: Vec::new(),
                updated: false,
            })
        }
    }
}

fn reconcile(
    tx: &Connection,
    repo_path: &str,
    chain: &db::Chain,
    now: jiff::Timestamp,
) -> Result<Recon> {
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
            return Ok(Recon {
                updated: false,
                warnings: Vec::new(),
            });
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
            return Ok(Recon {
                updated: true,
                warnings: Vec::new(),
            });
        }
    }

    // Step 2/3: classify fixups and attach them autosquash-style.
    let metas: Vec<CommitMeta> = commits
        .iter()
        .map(|c| CommitMeta {
            sha: c.id().to_string(),
            subject: fixup::subject_of(&String::from_utf8_lossy(c.message_bytes())),
        })
        .collect();
    let messages: Vec<String> = commits
        .iter()
        .map(|c| String::from_utf8_lossy(c.message_bytes()).into_owned())
        .collect();
    let resolver = |needle: &str| {
        repo.revparse_single(needle)
            .ok()
            .and_then(|o| o.peel_to_commit().ok())
            .map(|c| c.id().to_string())
    };
    let roots = fixup::attach_fixups(&metas, resolver);

    let mut warnings: Vec<String> = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        if root.is_some() && fixup::classify(&metas[i].subject) == Some(fixup::FixupKind::Squash) {
            warnings.push(format!(
                "squash! commit {} folded as a plain fixup (its message edits need an \
                 interactive rebase) — prefer fixup!",
                &metas[i].sha[..12]
            ));
        }
    }

    let regulars: Vec<usize> = (0..commits.len()).filter(|&i| roots[i].is_none()).collect();
    let mut fixups_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, root) in roots.iter().enumerate() {
        if let Some(r) = *root {
            fixups_of.entry(r).or_default().push(i);
        }
    }

    // Identity rule 1: Change-Id trailers, duplicates get derived keys.
    let trailers: Vec<Option<String>> = regulars
        .iter()
        .map(|&i| identity::change_id_trailer(&messages[i]))
        .collect();
    let short_shas: Vec<String> = regulars
        .iter()
        .map(|&i| metas[i].sha[..12].to_string())
        .collect();
    let (keys, dup_warnings) = identity::assign_trailer_keys(&trailers, &short_shas);
    warnings.extend(dup_warnings);

    // Step 4: match regular commits to change rows.
    let existing = db::changes_for_chain(tx, chain.id)?; // live by position, orphans last
    let mut latest_by_change: HashMap<i64, Option<db::Revision>> = HashMap::new();
    for row in &existing {
        latest_by_change.insert(row.id, db::latest_revision(tx, row.id)?);
    }
    let matched = match_changes(MatchInput {
        repo: &repo,
        commits: &commits,
        metas: &metas,
        regulars: &regulars,
        keys: &keys,
        existing: &existing,
        latest_by_change: &latest_by_change,
    });

    // Steps 5/6: per live change, fold the effective tree and insert a new
    // revision when the effective state moved.
    let claimed: HashSet<usize> = matched.iter().flatten().copied().collect();
    for (ri, &ci) in regulars.iter().enumerate() {
        let commit = &commits[ci];
        let position = ri as i64;
        let fix_indices = fixups_of.get(&ci).cloned().unwrap_or_default();
        let fix_commits: Vec<&Commit> = fix_indices.iter().map(|&i| &commits[i]).collect();
        let fixup_rows: Vec<db::Fixup> = fix_indices
            .iter()
            .map(|&i| db::Fixup {
                sha: metas[i].sha.clone(),
                message: messages[i].clone(),
            })
            .collect();

        let (change_id, stored_position, stored_status, mut status, is_new) = match matched[ri] {
            Some(ei) => {
                let row = &existing[ei];
                let status = if row.status == ChangeStatus::Orphaned {
                    // Re-attachment: status returns to its pre-orphan value,
                    // re-derived from the review history.
                    pre_orphan_status(tx, &repo, row.id)?
                } else {
                    row.status
                };
                (row.id, row.position, row.status, status, false)
            }
            None => {
                let base_key = keys[ri].clone().unwrap_or_else(|| metas[ci].sha.clone());
                let key = unique_key(tx, chain.id, &base_key)?;
                let row = db::insert_change(tx, chain.id, &key, position, ChangeStatus::Pending)?;
                updated = true;
                (
                    row.id,
                    Some(position),
                    ChangeStatus::Pending,
                    ChangeStatus::Pending,
                    true,
                )
            }
        };

        let latest = latest_by_change.get(&change_id).cloned().flatten();
        let same_state = latest.as_ref().is_some_and(|l| {
            l.commit_sha == metas[ci].sha
                && l.fixups
                    .iter()
                    .map(|f| &f.sha)
                    .eq(fixup_rows.iter().map(|f| &f.sha))
        });
        if !same_state {
            let eff = fold::effective_tree(&repo, commit, &fix_commits)?;
            let number = latest.as_ref().map_or(1, |l| l.number + 1);
            let parent_sha = commit.parent_id(0)?.to_string();
            db::insert_revision(
                tx,
                change_id,
                number,
                &metas[ci].sha,
                &parent_sha,
                eff.map(|o| o.to_string()).as_deref(),
                &fixup_rows,
                &messages[ci],
                &now_str,
            )?;
            updated = true;
            // Rule 6 status effect: pure rebase keeps status; anything
            // else means the reviewer must look again.
            let pure = latest.as_ref().is_some_and(|l| {
                pure_rebase_equivalent(&repo, &l.commit_sha, &l.fixups, &metas[ci].sha, &fixup_rows)
            });
            if !pure {
                status = ChangeStatus::Pending;
            }
        }

        if !is_new && (stored_position != Some(position) || stored_status != status) {
            db::change_set_position_status(tx, change_id, Some(position), status)?;
            updated = true;
        }
    }

    // Step 4, orphaning: live rows whose commit matched nothing. Never
    // deleted — comments, drafts and reviews stay.
    for (ei, row) in existing.iter().enumerate() {
        if !claimed.contains(&ei) && row.status != ChangeStatus::Orphaned {
            db::change_set_position_status(tx, row.id, None, ChangeStatus::Orphaned)?;
            updated = true;
        }
    }

    // GC safety: keep refs for every revision; re-fold vanished trees.
    for row in db::changes_for_chain(tx, chain.id)? {
        for rev in db::revisions_for_change(tx, row.id)? {
            let rev = repair_effective_tree(tx, &repo, rev)?;
            fold::ensure_keep_ref(&repo, chain.id, row.id, &rev);
        }
    }

    // Step 7: net structural difference → one chain_updated event.
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
    Ok(Recon { updated, warnings })
}

/// Walk `base..tip` oldest-first. Any merge commit aborts the scan with
/// the documented error; so does a root commit (the diff/identity model
/// needs a first parent everywhere).
fn walk_chain<'r>(repo: &'r Repository, base: Oid, tip: Oid) -> Result<Vec<Commit<'r>>> {
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
) -> Result<Recon> {
    if chain.status != ChainStatus::Active {
        return Ok(Recon {
            updated: false,
            warnings: Vec::new(),
        });
    }
    let marker = format!("branch '{}' not found", chain.branch);
    if chain.last_scan_error.as_deref() == Some(marker.as_str())
        && let Ok(prev) = chain.updated_at.parse::<jiff::Timestamp>()
        && now.as_second() - prev.as_second() >= 10
    {
        close_chain(tx, repo, chain, ChainStatus::Abandoned, &now.to_string())?;
        return Ok(Recon {
            updated: true,
            warnings: Vec::new(),
        });
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
    fold::delete_chain_keep_refs(repo, chain.id);
    Ok(())
}

/// Merged quorum (step 1): every live non-empty change's effective
/// patch-id must appear in `fork..base`, where fork is the chain's
/// recorded fork point (the first live change's parent). At least one
/// non-empty live change must vote, else `tip == base` is just an empty
/// active chain. Anything unverifiable counts against merging.
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
            && let Ok(pid) = fold::tree_patch_id(repo, &parent_tree, &tree)
        {
            base_patch_ids.insert(pid);
        }
    }

    // A change matches by Change-Id trailer first (immune to the patch-id
    // context drift an autosquash of a *neighboring* change causes), then
    // by folded patch-id. Empty diffs are trivially matched but don't
    // count toward the quorum.
    let mut any_matched = false;
    for (key, rev) in &candidates {
        if base_trailers.contains(key) {
            any_matched = true;
            continue;
        }
        match revision_effective_patch_id(repo, rev) {
            Some(pid) if pid == fold::EMPTY_PATCH_ID => continue,
            Some(pid) if base_patch_ids.contains(&pid) => any_matched = true,
            // Unverifiable (conflicted fold, pruned objects) or unmatched.
            _ => return Ok(false),
        }
    }
    Ok(any_matched)
}

/// Patch-id of a revision's reviewed diff: `parent_sha → effective_tree`.
fn revision_effective_patch_id(repo: &Repository, rev: &db::Revision) -> Option<String> {
    let parent = repo
        .find_commit(Oid::from_str(&rev.parent_sha).ok()?)
        .ok()?;
    let eff = rev.effective_tree.as_deref()?;
    let tree = repo.find_tree(Oid::from_str(eff).ok()?).ok()?;
    fold::tree_patch_id(repo, &parent.tree().ok()?, &tree).ok()
}

struct MatchInput<'a, 'r> {
    repo: &'r Repository,
    commits: &'a [Commit<'r>],
    metas: &'a [CommitMeta],
    regulars: &'a [usize],
    keys: &'a [Option<String>],
    existing: &'a [db::Change],
    latest_by_change: &'a HashMap<i64, Option<db::Revision>>,
}

/// Identity matching, in contract priority order across the whole scan:
/// one pass per rule, each pass in walk order, each row claimable once.
/// Returns, per regular commit, the index into `existing` it matched.
fn match_changes(input: MatchInput) -> Vec<Option<usize>> {
    let MatchInput {
        repo,
        commits,
        metas,
        regulars,
        keys,
        existing,
        latest_by_change,
    } = input;
    let latest = |ei: usize| {
        latest_by_change
            .get(&existing[ei].id)
            .and_then(|r| r.as_ref())
    };
    let mut matched: Vec<Option<usize>> = vec![None; regulars.len()];
    let mut claimed: HashSet<usize> = HashSet::new();
    let claim =
        |matched: &mut Vec<Option<usize>>, claimed: &mut HashSet<usize>, ri: usize, ei: usize| {
            matched[ri] = Some(ei);
            claimed.insert(ei);
        };

    // Rule 1: Change-Id trailer (including derived duplicate keys).
    for (ri, key) in keys.iter().enumerate() {
        if let Some(key) = key
            && let Some(ei) = existing.iter().position(|row| &row.change_key == key)
            && !claimed.contains(&ei)
        {
            claim(&mut matched, &mut claimed, ri, ei);
        }
    }

    // Rule 2: exact sha — commit unchanged since last scan.
    for ri in 0..regulars.len() {
        if matched[ri].is_some() {
            continue;
        }
        let sha = &metas[regulars[ri]].sha;
        if let Some(ei) = (0..existing.len()).find(|&ei| {
            !claimed.contains(&ei) && latest(ei).is_some_and(|rev| &rev.commit_sha == sha)
        }) {
            claim(&mut matched, &mut claimed, ri, ei);
        }
    }

    // Rule 3: patch-id — same diff, new sha. Row patch-ids come from the
    // stored shas (pinned by keep refs); unverifiable rows never match.
    let mut row_pid_cache: HashMap<usize, Option<String>> = HashMap::new();
    let mut row_pid = |ei: usize| -> Option<String> {
        row_pid_cache
            .entry(ei)
            .or_insert_with(|| {
                let rev = latest(ei)?;
                let parent = repo
                    .find_commit(Oid::from_str(&rev.parent_sha).ok()?)
                    .ok()?;
                let commit = repo
                    .find_commit(Oid::from_str(&rev.commit_sha).ok()?)
                    .ok()?;
                fold::tree_patch_id(repo, &parent.tree().ok()?, &commit.tree().ok()?).ok()
            })
            .clone()
    };
    for ri in 0..regulars.len() {
        if matched[ri].is_some() {
            continue;
        }
        let Ok(pid) = fold::commit_patch_id(repo, &commits[regulars[ri]]) else {
            continue;
        };
        if let Some(ei) = (0..existing.len())
            .find(|&ei| !claimed.contains(&ei) && row_pid(ei).as_deref() == Some(pid.as_str()))
        {
            claim(&mut matched, &mut claimed, ri, ei);
        }
    }

    // Rule 4: subject — only against changes that were live at scan start
    // (whose commit left the branch); orphans don't subject-match.
    for ri in 0..regulars.len() {
        if matched[ri].is_some() {
            continue;
        }
        let subject = &metas[regulars[ri]].subject;
        if let Some(ei) = (0..existing.len()).find(|&ei| {
            !claimed.contains(&ei)
                && existing[ei].status != ChangeStatus::Orphaned
                && latest(ei).is_some_and(|rev| &fixup::subject_of(&rev.message) == subject)
        }) {
            claim(&mut matched, &mut claimed, ri, ei);
        }
    }

    matched
}

/// True when two effective states differ only by a rebase: same fixup
/// count, patch-id-equal commit, pairwise patch-id-equal fixups (rule 6;
/// the same predicate is behind review auto-retargeting in api.md).
/// Unverifiable objects make it false — the reviewer looks again.
pub fn pure_rebase_equivalent(
    repo: &Repository,
    old_commit_sha: &str,
    old_fixups: &[db::Fixup],
    new_commit_sha: &str,
    new_fixups: &[db::Fixup],
) -> bool {
    if old_fixups.len() != new_fixups.len() {
        return false;
    }
    let pid = |sha: &str| -> Option<String> {
        let commit = repo.find_commit(Oid::from_str(sha).ok()?).ok()?;
        fold::commit_patch_id(repo, &commit).ok()
    };
    let eq = |a: &str, b: &str| a == b || matches!((pid(a), pid(b)), (Some(x), Some(y)) if x == y);
    eq(old_commit_sha, new_commit_sha)
        && old_fixups
            .iter()
            .zip(new_fixups)
            .all(|(o, n)| eq(&o.sha, &n.sha))
}

/// A change's pre-orphan status, re-derived from review history — exactly
/// what the status machine would have produced: a review counts if it sits
/// on the latest revision *or* on any older revision connected to it by an
/// unbroken run of pure rebases (rule 6 keeps status across those, and the
/// review row stays on the old revision number).
fn pre_orphan_status(tx: &Connection, repo: &Repository, change_id: i64) -> Result<ChangeStatus> {
    let revisions = db::revisions_for_change(tx, change_id)?; // ascending
    let Some(latest) = revisions.last() else {
        return Ok(ChangeStatus::Pending);
    };
    let mut eligible = vec![latest.number];
    let mut newer = latest;
    for older in revisions.iter().rev().skip(1) {
        if !pure_rebase_equivalent(
            repo,
            &older.commit_sha,
            &older.fixups,
            &newer.commit_sha,
            &newer.fixups,
        ) {
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

/// First free change key: `base`, else `base#2`, `base#3`, … (collisions
/// only happen for sha-derived keys of long-orphaned rows).
fn unique_key(tx: &Connection, chain_id: i64, base: &str) -> Result<String> {
    if !db::change_key_exists(tx, chain_id, base)? {
        return Ok(base.to_string());
    }
    for n in 2.. {
        let key = format!("{base}#{n}");
        if !db::change_key_exists(tx, chain_id, &key)? {
            return Ok(key);
        }
    }
    unreachable!()
}

/// Re-fold a revision whose effective tree object vanished (gc despite
/// keep refs, or keep refs dropped at close before a reopen). Best-effort.
fn repair_effective_tree(
    tx: &Connection,
    repo: &Repository,
    rev: db::Revision,
) -> Result<db::Revision> {
    let Some(tree_sha) = rev.effective_tree.as_deref() else {
        return Ok(rev);
    };
    if Oid::from_str(tree_sha)
        .ok()
        .and_then(|oid| repo.find_tree(oid).ok())
        .is_some()
    {
        return Ok(rev);
    }
    let Some(commit) = Oid::from_str(&rev.commit_sha)
        .ok()
        .and_then(|oid| repo.find_commit(oid).ok())
    else {
        return Ok(rev); // original pruned too; history is best-effort now
    };
    let mut fix_commits = Vec::new();
    for fixup in &rev.fixups {
        match Oid::from_str(&fixup.sha)
            .ok()
            .and_then(|oid| repo.find_commit(oid).ok())
        {
            Some(c) => fix_commits.push(c),
            None => return Ok(rev),
        }
    }
    let fix_refs: Vec<&Commit> = fix_commits.iter().collect();
    if let Some(oid) = fold::effective_tree(repo, &commit, &fix_refs)? {
        let oid = oid.to_string();
        db::revision_set_effective_tree(tx, rev.id, Some(&oid))?;
        return Ok(db::Revision {
            effective_tree: Some(oid),
            ..rev
        });
    }
    Ok(rev)
}
