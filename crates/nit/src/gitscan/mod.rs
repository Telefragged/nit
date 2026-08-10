//! The git layer: the push walk and merged/abandoned detection.
//!
//! The detection serves the background timer; [`walk_push`] and
//! [`detect_merges`] carry the contract.
//!
//! Everything here is pure with respect to the database: it reads git and
//! returns values the caller (the api layer) folds into the per-change logs.
//! Keep refs (GC safety) are an idempotent side effect.
//!
//! - [`identity`] — `Change-Id:` trailer extraction and validation.
//! - [`objects`] — patch-ids and GC-safety keep refs.

pub mod identity;
pub mod objects;

use std::collections::HashMap;

use git2::{Commit, Oid, Repository, Sort};

use nit_types::domain::{ChangeId, ChangeNumber, Sha};
use nit_types::fold::subject_of;

use crate::review::ChangeProjection;

pub const MERGE_COMMIT_ERROR: &str = "chain contains merge commits — rebase onto the base instead";

/// A commit sha truncated to 12 chars — the canonical short form for display.
#[must_use]
pub fn short_sha(sha: &Sha) -> String {
    sha.as_str().chars().take(12).collect()
}

/// One commit the push walk recorded, oldest-first.
///
/// `parent_sha` is its first parent — the previous member, or the fork for
/// the first.
#[derive(Debug, Clone)]
pub struct WalkedCommit {
    pub change_key: ChangeId,
    pub commit_sha: Sha,
    pub parent_sha: Sha,
    pub message: String,
}

/// A push walk's fork point and the commits between it and the tip.
///
/// The fork point is on the canonical ref; the commits are
/// oldest-first.
#[derive(Debug, Clone)]
pub struct PushWalk {
    pub fork_sha: Sha,
    pub commits: Vec<WalkedCommit>,
}

fn resolve_commit(repo: &Repository, refish: &str) -> Result<Oid, String> {
    repo.revparse_single(refish)
        .and_then(|o| o.peel_to_commit())
        .map(|c| c.id())
        .map_err(|e| format!("cannot resolve '{refish}': {}", e.message()))
}

/// Walks `merge-base(base, tip)..tip` oldest-first and validates it.
///
/// The whole walk is all-or-nothing: any structural fault is an
/// `Err(message)` the caller maps to a 400.
///
/// # Errors
///
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
        .map(|c| short_sha(&Sha::from(c.id().to_string())))
        .collect();
    let keys = identity::require_keys(&messages, &short_shas)?;

    let mut walked = Vec::with_capacity(commits.len());
    let mut prev = Sha::from(fork.to_string());
    for (i, commit) in commits.iter().enumerate() {
        let sha = Sha::from(commit.id().to_string());
        walked.push(WalkedCommit {
            change_key: keys[i].clone().into(),
            commit_sha: sha.clone(),
            parent_sha: prev.clone(),
            message: messages[i].clone(),
        });
        prev = sha;
    }
    Ok(PushWalk {
        fork_sha: Sha::from(fork.to_string()),
        commits: walked,
    })
}

/// Walks `base..tip` oldest-first, rejecting merge and root commits.
///
/// The diff/identity model needs a single first parent everywhere.
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

/// True when a revision differs from its predecessor only by a rebase.
///
/// That is a patch-id-equal commit with an unchanged message.
/// Unverifiable objects make it false.
#[must_use]
pub fn pure_rebase(
    repo: &Repository,
    old_sha: &Sha,
    old_msg: &str,
    new_sha: &Sha,
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

/// The canonical ref's current HEAD sha.
///
/// `None` when it can't be resolved (the merge timer's per-sweep baseline
/// check).
#[must_use]
pub fn resolve_head(repo: &Repository, canonical_ref: &str) -> Option<Sha> {
    Some(Sha::from(
        resolve_commit(repo, canonical_ref).ok()?.to_string(),
    ))
}

/// Landings observed on the canonical ref in `since..head`.
///
/// That window is the commits added since the last sweep: each open
/// change whose `Change-Id` appears on a new single-parent commit, paired
/// with the merged commit's sha. One walk covers every change; `open`
/// maps `change_key →` the change. At most one merge per change.
///
/// A merge that *stripped* its Change-Id is not detected — nit's own approve
/// action preserves the trailer through rebase + fast-forward, and chasing
/// keyless landings is what forced an unbounded per-change diff every sweep.
#[must_use]
pub fn detect_merges<S: std::hash::BuildHasher>(
    repo: &Repository,
    since: &Sha,
    head: &Sha,
    open: &HashMap<ChangeId, &ChangeProjection, S>,
) -> Vec<(ChangeNumber, Sha)> {
    let (Ok(since), Ok(head)) = (Oid::from_str(since.as_str()), Oid::from_str(head.as_str()))
    else {
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

    let mut landings: HashMap<ChangeNumber, Sha> = HashMap::new();
    for oid in walk.flatten() {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        if commit.parent_count() != 1 {
            continue;
        }
        let Some(key) =
            identity::change_id_trailer(&String::from_utf8_lossy(commit.message_bytes()))
        else {
            continue;
        };
        let Some(change) = open.get(&ChangeId::from(key.clone())) else {
            continue;
        };
        // First seen wins: the unsorted walk is newest-first, so a key
        // appearing on several new commits records the newest merge.
        landings
            .entry(change.id)
            .or_insert_with(|| Sha::from(oid.to_string()));
    }
    landings.into_iter().collect()
}

/// One walked commit of the canonical ref.
///
/// `trailer` is the commit's raw `Change-Id:` trailer when present; the api
/// layer resolves it to the change it names when building the wire shape.
#[derive(Debug, Clone)]
pub struct HistoryCommit {
    pub sha: Sha,
    pub parents: Vec<Sha>,
    pub subject: String,
    pub trailer: Option<String>,
}

/// Walks the canonical ref from its HEAD, newest-first.
///
/// The HEAD commit (the graph anchor) followed by up to `window` ancestor
/// commits — the merged history that descends below HEAD. Topological, so
/// every commit precedes its parents; a merge keeps both parents (the
/// client draws edges only to the parents inside the window). The
/// returned bool is `truncated`: the branch has at least one more merged
/// commit below the window (the client shows an "earlier history hidden"
/// marker and dangles deep forks to it).
///
/// # Errors
///
/// When the canonical ref can't be resolved or the walk fails.
pub fn canonical_history(
    repo: &Repository,
    canonical_ref: &str,
    window: u64,
) -> Result<(Vec<HistoryCommit>, bool), String> {
    let head = resolve_commit(repo, canonical_ref)?;
    let mut walk = repo.revwalk().map_err(|e| e.to_string())?;
    walk.push(head).map_err(|e| e.to_string())?;
    walk.set_sorting(Sort::TOPOLOGICAL)
        .map_err(|e| e.to_string())?;
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
            sha: oid.to_string().into(),
            parents: commit
                .parent_ids()
                .map(|p| Sha::from(p.to_string()))
                .collect(),
            subject: subject_of(&message),
            trailer: identity::change_id_trailer(&message),
        });
    }
    Ok((out, truncated))
}

/// The keep-ref maintenance for one change's revisions — idempotent.
pub fn maintain_keep_refs(repo: &Repository, change: &ChangeProjection) {
    for revision in &change.revisions {
        objects::ensure_keep_ref(repo, change.id, revision.number, &revision.commit_sha);
    }
}

#[cfg(test)]
mod tests {
    use nit_types::domain::RevisionNumber;
    use std::collections::HashMap;

    use git2::{Oid, Repository, Signature};

    use super::detect_merges;
    use crate::review::{ChangeProjection, RevisionProjection};
    use nit_types::domain::{ChangeId, ChangeNumber, Sha};

    /// Flat paths only — a `TreeBuilder` seeded from the parent is all these
    /// tests need.
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

    fn keyed(subject: &str, key: &str) -> String {
        format!("{subject}\n\nChange-Id: {key}\n")
    }

    fn change_proj(id: u64, key: &str, commit: Oid, base: Oid) -> ChangeProjection {
        let mut proj = ChangeProjection::new(ChangeNumber(id), 1, key.into());
        proj.revisions.push(RevisionProjection {
            number: RevisionNumber(0),
            commit_sha: commit.to_string().into(),
            parent_sha: base.to_string().into(),
            fork_sha: base.to_string().into(),
            message: keyed("subject", key),
            resets_status: true,
            created_at: "t0".to_string(),
        });
        proj
    }

    fn repo() -> (tempfile::TempDir, Repository, Oid) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        let root = commit(&repo, None, "init\n", &[("README", "hello\n")]);
        (dir, repo, root)
    }

    fn open<'a>(changes: &[&'a ChangeProjection]) -> HashMap<ChangeId, &'a ChangeProjection> {
        changes.iter().map(|c| (c.change_key.clone(), *c)).collect()
    }

    // The positive single-match path is covered by `stacked_prefix_detects_…`
    // below (which lands one commit per change through the same logic) and
    // end-to-end by `change_landed_on_main_becomes_merged`; the tests here
    // pin the branches those don't reach — and `drifted_landing_is_detected`
    // pins the walk's content-blindness (no fixture elsewhere lands a diff
    // that matches no pushed revision).

    /// The merge rebase may adapt the diff, so the merged content can
    /// match no pushed revision; the Change-Id still identifies the merge
    /// and the merged commit's sha is recorded.
    #[test]
    fn drifted_landing_is_detected() {
        let (_dir, repo, root) = repo();
        let feat = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a\n")],
        );
        let change = change_proj(1, "I001", feat, root);
        let merged = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a adapted\n")],
        );
        let got = detect_merges(
            &repo,
            &Sha::from(root.to_string()),
            &Sha::from(merged.to_string()),
            &open(&[&change]),
        );
        assert_eq!(got, vec![(ChangeNumber(1), Sha::from(merged.to_string()))]);
    }

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
        let merged = commit(
            &repo,
            Some(root),
            "merged without a trailer\n",
            &[("a.txt", "a\n")],
        );
        let got = detect_merges(
            &repo,
            &Sha::from(root.to_string()),
            &Sha::from(merged.to_string()),
            &open(&[&change]),
        );
        assert_eq!(got, vec![]);
    }

    /// One delta walk detects every member that merged — a stacked prefix
    /// (A and B merge, each at its own revision) falls out for free.
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
        let landed_a = commit(&repo, Some(root), &keyed("a", "I001"), &[("a.txt", "a\n")]);
        let landed_b = commit(
            &repo,
            Some(landed_a),
            &keyed("b", "I002"),
            &[("b.txt", "b\n")],
        );
        let mut got = detect_merges(
            &repo,
            &Sha::from(root.to_string()),
            &Sha::from(landed_b.to_string()),
            &open(&[&a, &b]),
        );
        got.sort_unstable();
        assert_eq!(
            got,
            vec![
                (ChangeNumber(1), Sha::from(landed_a.to_string())),
                (ChangeNumber(2), Sha::from(landed_b.to_string()))
            ]
        );
    }

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
        let merged = commit(
            &repo,
            Some(root),
            &keyed("other", "I999"),
            &[("z.txt", "z\n")],
        );
        let got = detect_merges(
            &repo,
            &Sha::from(root.to_string()),
            &Sha::from(merged.to_string()),
            &open(&[&change]),
        );
        assert_eq!(got, vec![]);
    }

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
        let merged = commit(
            &repo,
            Some(root),
            &keyed("feat", "I001"),
            &[("a.txt", "a\n")],
        );
        let absent = Sha::from("0".repeat(40));
        let got = detect_merges(
            &repo,
            &absent,
            &Sha::from(merged.to_string()),
            &open(&[&change]),
        );
        assert_eq!(got, vec![]);
    }
}
