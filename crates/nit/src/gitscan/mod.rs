//! The git layer: the push walk and merged/abandoned detection for the
//! background timer — docs/data-model.md ("Push", "Lifecycle") is the
//! contract.
//!
//! Everything here is pure with respect to the database: it reads git and
//! returns values the caller (the api layer) folds into the per-change logs.
//! Keep refs (GC safety) are an idempotent side effect.
//!
//! - [`identity`] — `Change-Id:` trailer extraction and validation.
//! - [`objects`] — patch-ids and GC-safety keep refs.

pub mod identity;
pub mod objects;

use std::collections::{HashMap, HashSet};

use git2::{Commit, Oid, Repository, Sort};

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

/// The canonical branch's current HEAD sha, or `None` when it can't be
/// resolved (the merge timer's per-sweep baseline check).
#[must_use]
pub fn resolve_head(repo: &Repository, base_ref: &str) -> Option<String> {
    Some(resolve_commit(repo, base_ref).ok()?.to_string())
}

/// Landings observed on the canonical branch in the window `since..head` (the
/// commits added since the last sweep): each open change whose `Change-Id`
/// appears on a new single-parent commit whose patch-id equals that change's
/// latest revision, paired with the landed revision number (docs/data-model.md
/// "Lifecycle timer"). One walk covers every change; `open` maps `change_key →`
/// the change. At most one landing per change.
///
/// A landing that *stripped* its Change-Id is not detected — nit's own approve
/// action preserves the trailer through rebase + fast-forward, and chasing
/// keyless landings is what forced an unbounded per-change diff every sweep.
#[must_use]
pub fn detect_landings<S: std::hash::BuildHasher>(
    repo: &Repository,
    since: &str,
    head: &str,
    open: &HashMap<String, &ChangeProj, S>,
) -> Vec<(u64, u64)> {
    let (Ok(since), Ok(head)) = (Oid::from_str(since), Oid::from_str(head)) else {
        return Vec::new();
    };
    let Ok(mut walk) = repo.revwalk() else {
        return Vec::new();
    };
    // A baseline that no longer resolves (a rewritten branch, a gc'd commit)
    // makes the delta undefined — re-baseline and detect nothing this sweep.
    if walk.push(head).is_err() || walk.hide(since).is_err() {
        return Vec::new();
    }

    let mut landings = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    for oid in walk.flatten() {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        // The diff/identity model is single-parent throughout; a merge commit
        // carries no patch-id to match.
        if commit.parent_count() != 1 {
            continue;
        }
        let Some(key) =
            identity::change_id_trailer(&String::from_utf8_lossy(commit.message_bytes()))
        else {
            continue;
        };
        let Some(change) = open.get(&key) else {
            continue;
        };
        if !seen.contains(&change.id)
            && let Some(revision) = landed_at(repo, change, &oid.to_string())
        {
            seen.insert(change.id);
            landings.push((change.id, revision));
        }
    }
    landings
}

/// The revision number the landed commit `sha` carries, if it is the change's
/// **latest** revision: a patch-id match against the latest ⇒ landed; a match
/// against an older revision is "landed earlier, since amended" and returns
/// `None` (the change stays open). An empty diff never counts.
fn landed_at(repo: &Repository, change: &ChangeProj, sha: &str) -> Option<u64> {
    let latest = change.latest_revision()?;
    let landed = objects::sha_patch_id(repo, sha)?;
    let target = objects::sha_patch_id(repo, &latest.commit_sha)?;
    (landed == target && landed != objects::EMPTY_PATCH_ID).then_some(latest.number)
}

/// One commit on the canonical branch, for the graph's HEAD anchor and merged
/// history (docs/api.md "Graph"). `parents` are all parent shas (a merge keeps
/// both); `change_key` is the commit's `Change-Id` trailer when present, used
/// to enrich the node from the matching change.
#[derive(Debug, Clone)]
pub struct HistoryCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub subject: String,
    pub change_key: Option<String>,
}

/// Walk the canonical branch from its HEAD: the HEAD commit (the graph anchor)
/// followed by up to `window` ancestor commits, newest-first — the merged
/// history that descends below HEAD. Topological, so every commit precedes its
/// parents; a merge keeps both parents (the client draws edges only to the
/// parents inside the window). The returned bool is `truncated`: the branch has
/// at least one more merged commit below the window (the client shows an
/// "earlier history hidden" marker and dangles deep forks to it).
///
/// # Errors
/// When the canonical branch can't be resolved or the walk fails.
pub fn canonical_history(
    repo: &Repository,
    base_ref: &str,
    window: u64,
) -> Result<(Vec<HistoryCommit>, bool), String> {
    let head = resolve_commit(repo, base_ref)?;
    let mut walk = repo.revwalk().map_err(|e| e.to_string())?;
    walk.push(head).map_err(|e| e.to_string())?;
    walk.set_sorting(Sort::TOPOLOGICAL)
        .map_err(|e| e.to_string())?;
    // The anchor plus `window` merged commits below it; one more means the
    // history is truncated.
    let take = usize::try_from(window)
        .unwrap_or(usize::MAX)
        .saturating_add(1);
    let mut out = Vec::new();
    let mut truncated = false;
    for oid in walk {
        let oid = oid.map_err(|e| e.to_string())?;
        if out.len() >= take {
            truncated = true;
            break;
        }
        let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
        let message = String::from_utf8_lossy(commit.message_bytes());
        out.push(HistoryCommit {
            sha: oid.to_string(),
            parents: commit.parent_ids().map(|p| p.to_string()).collect(),
            subject: identity::subject_of(&message),
            change_key: identity::change_id_trailer(&message),
        });
    }
    Ok((out, truncated))
}

/// The keep-ref maintenance for one change's revisions — idempotent
/// (docs/data-model.md "Keep refs").
pub fn maintain_keep_refs(repo: &Repository, change: &ChangeProj) {
    for rev in &change.revisions {
        objects::ensure_keep_ref(repo, change.id, rev.number, &rev.commit_sha);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use git2::{Oid, Repository, Signature};

    use super::detect_landings;
    use crate::db::ChangeRow;
    use crate::review::{ChangeProj, RevisionProj};

    /// Commit `files` onto `parent` (none → root) with `message`, returning the
    /// new commit's sha. Flat paths only — a `TreeBuilder` seeded from the
    /// parent is all these tests need.
    fn commit(
        repo: &Repository,
        parent: Option<Oid>,
        message: &str,
        files: &[(&str, &str)],
    ) -> Oid {
        let parent_commit = parent.map(|p| repo.find_commit(p).expect("find parent"));
        let base_tree = parent_commit
            .as_ref()
            .map(|c| c.tree().expect("parent tree"));
        let mut builder = repo.treebuilder(base_tree.as_ref()).expect("treebuilder");
        for (path, content) in files {
            let blob = repo.blob(content.as_bytes()).expect("write blob");
            builder.insert(path, blob, 0o100_644).expect("insert");
        }
        let tree = repo
            .find_tree(builder.write().expect("write tree"))
            .expect("find tree");
        let sig = Signature::new("t", "t@e", &git2::Time::new(0, 0)).expect("signature");
        let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
        repo.commit(None, &sig, &sig, message, &tree, &parents)
            .expect("commit")
    }

    /// `subject` + a `Change-Id` trailer.
    fn keyed(subject: &str, key: &str) -> String {
        format!("{subject}\n\nChange-Id: {key}\n")
    }

    /// A one-revision change `id`/`key`, forked at `base`, tipped at `commit`.
    fn change_proj(id: u64, key: &str, commit: Oid, base: Oid) -> ChangeProj {
        let mut proj = ChangeProj::empty(&ChangeRow {
            id,
            repo_id: 1,
            change_key: key.to_string(),
            status: None,
            created_at: "t0".to_string(),
        });
        proj.revisions.push(RevisionProj {
            number: 0,
            commit_sha: commit.to_string(),
            parent_sha: base.to_string(),
            base_sha: base.to_string(),
            message: keyed("subject", key),
            partial: false,
            resets_status: true,
            created_at: "t0".to_string(),
        });
        proj
    }

    /// A fresh repo with a root commit. Returns `(dir, repo, root)`.
    fn repo() -> (tempfile::TempDir, Repository, Oid) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        let root = commit(&repo, None, "init\n", &[("README", "hello\n")]);
        (dir, repo, root)
    }

    /// The open-change lookup `detect_landings` takes (`change_key` → change).
    fn open<'a>(changes: &[&'a ChangeProj]) -> HashMap<String, &'a ChangeProj> {
        changes.iter().map(|c| (c.change_key.clone(), *c)).collect()
    }

    // The positive single-match path is covered by `stacked_prefix_detects_…`
    // below (which lands one commit per change through the same logic) and
    // end-to-end by `change_landed_on_main_becomes_merged`; the tests here pin
    // the branches those don't reach.

    /// A landing carrying the key but a *different* patch-id (the revision was
    /// amended after an earlier patchset landed) keeps the change open.
    #[test]
    fn amended_since_is_not_detected() {
        let (_dir, repo, root) = repo();
        let feat = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a\n")],
        );
        let change = change_proj(1, "I001", feat, root);
        let landed = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "b\n")],
        );
        let got = detect_landings(
            &repo,
            &root.to_string(),
            &landed.to_string(),
            &open(&[&change]),
        );
        assert_eq!(got, vec![]);
    }

    /// A landing that stripped its Change-Id is not detected — the keyless
    /// patch-id fallback is deliberately gone.
    #[test]
    fn keyless_landing_is_not_detected() {
        let (_dir, repo, root) = repo();
        let feat = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a\n")],
        );
        let change = change_proj(1, "I001", feat, root);
        let landed = commit(
            &repo,
            Some(root),
            "landed without a trailer\n",
            &[("a.txt", "a\n")],
        );
        let got = detect_landings(
            &repo,
            &root.to_string(),
            &landed.to_string(),
            &open(&[&change]),
        );
        assert_eq!(got, vec![]);
    }

    /// An empty-diff revision never counts as landed, even against an identical
    /// empty commit carrying the key.
    #[test]
    fn empty_diff_is_not_detected() {
        let (_dir, repo, root) = repo();
        let noop = commit(&repo, Some(root), &keyed("noop", "I001"), &[]);
        let change = change_proj(1, "I001", noop, root);
        let landed = commit(&repo, Some(root), &keyed("noop", "I001"), &[]);
        let got = detect_landings(
            &repo,
            &root.to_string(),
            &landed.to_string(),
            &open(&[&change]),
        );
        assert_eq!(got, vec![]);
    }

    /// One delta walk detects every member that landed — a stacked prefix
    /// (A and B land, each at its own revision) falls out for free.
    #[test]
    fn stacked_prefix_detects_each_member() {
        let (_dir, repo, root) = repo();
        let a_feat = commit(&repo, Some(root), &keyed("a", "I001"), &[("a.txt", "a\n")]);
        let b_feat = commit(
            &repo,
            Some(a_feat),
            &keyed("b", "I002"),
            &[("b.txt", "b\n")],
        );
        let a = change_proj(1, "I001", a_feat, root);
        let b = change_proj(2, "I002", b_feat, a_feat);
        // Both land on a fresh line off the root.
        let landed_a = commit(&repo, Some(root), &keyed("a", "I001"), &[("a.txt", "a\n")]);
        let landed_b = commit(
            &repo,
            Some(landed_a),
            &keyed("b", "I002"),
            &[("b.txt", "b\n")],
        );
        let mut got = detect_landings(
            &repo,
            &root.to_string(),
            &landed_b.to_string(),
            &open(&[&a, &b]),
        );
        got.sort_unstable();
        assert_eq!(got, vec![(1, 0), (2, 0)]);
    }

    /// A landing whose Change-Id matches no open change is ignored.
    #[test]
    fn commit_outside_the_open_set_is_ignored() {
        let (_dir, repo, root) = repo();
        let feat = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a\n")],
        );
        let change = change_proj(1, "I001", feat, root);
        let landed = commit(
            &repo,
            Some(root),
            &keyed("other", "I999"),
            &[("z.txt", "z\n")],
        );
        let got = detect_landings(
            &repo,
            &root.to_string(),
            &landed.to_string(),
            &open(&[&change]),
        );
        assert_eq!(got, vec![]);
    }

    /// A baseline that no longer resolves yields no landings (the caller
    /// re-baselines and detects nothing that sweep).
    #[test]
    fn unresolvable_baseline_detects_nothing() {
        let (_dir, repo, root) = repo();
        let feat = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a\n")],
        );
        let change = change_proj(1, "I001", feat, root);
        let landed = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a\n")],
        );
        let absent = "0".repeat(40);
        let got = detect_landings(&repo, &absent, &landed.to_string(), &open(&[&change]));
        assert_eq!(got, vec![]);
    }
}
