//! Chain lifecycle: merged detection (patch-id quorum), the
//! tip==base-is-not-merged rule, the two-scan abandoned rule, reopening,
//! and keep-ref cleanup on close.

mod common;

use common::{Fixture, msg};
use nit::enums::ChainStatus;
use nit::review::Status;

fn ts(secs: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_second(secs).unwrap()
}

fn keep_ref_count(f: &Fixture) -> usize {
    f.repo
        .references_glob(&format!("refs/nit/keep/{}/*", f.proj.chain_id))
        .unwrap()
        .count()
}

#[test]
fn fast_forward_merge_closes_chain() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    assert_eq!(keep_ref_count(&f), 2);

    // The agent fast-forwards main to the chain tip.
    f.branch("main", c2);
    assert!(!f.scan().entries.is_empty());
    assert_eq!(f.status(), ChainStatus::Merged);
    assert_eq!(f.appended("chain_closed"), 1);
    assert_eq!(keep_ref_count(&f), 0, "keep refs deleted on close");

    // Changes stay untouched (review history preserved).
    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|c| !c.orphaned));

    // Idempotent: scanning a closed chain again is a no-op.
    assert!(f.scan().entries.is_empty());
    assert_eq!(f.appended("chain_closed"), 1);
}

#[test]
fn merge_commit_on_main_closes_chain() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();

    // True merge commit on main; feat still points at c2.
    let m = f.commit(&[f.root, c2], "Merge feat\n", &[]);
    f.branch("main", m);
    f.scan();
    assert_eq!(f.status(), ChainStatus::Merged);
}

#[test]
fn squash_merge_of_single_change_closes_chain() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    // Squash-merge: same diff under a new commit, then the agent resets the
    // branch onto main. The patch-id quorum recognises the content.
    let s = f.commit(&[f.root], "one (squashed)\n", &[("a.txt", "a\n")]);
    f.branch("main", s);
    f.branch("feat", s);
    f.scan();
    assert_eq!(f.status(), ChainStatus::Merged);
}

#[test]
fn reset_to_base_is_an_empty_active_chain_not_merged() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();

    // tip == base but nothing landed in main: an agent rebuild, not a merge.
    f.branch("feat", f.root);
    f.scan();
    assert_eq!(f.status(), ChainStatus::Active);
    assert_eq!(f.appended("chain_closed"), 0);
    assert!(f.changes().iter().all(|c| c.orphaned));
}

#[test]
fn merged_chain_reopens_on_new_commits() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    f.branch("main", c1); // ff merge
    f.scan();
    assert_eq!(f.status(), ChainStatus::Merged);

    // New work on the same branch name reopens the chain.
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    assert!(!f.scan().entries.is_empty());
    assert_eq!(f.status(), ChainStatus::Active);

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert!(f.change("I001").orphaned, "merged work left the walk");
    assert_eq!(f.change("I002").position, Some(0));
    assert_eq!(
        keep_ref_count(&f),
        2,
        "keep refs re-created for every revision"
    );
}

#[test]
fn abandoned_only_after_two_scans_ten_seconds_apart() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan_at(ts(1_000));
    f.delete_branch("feat");

    // First missing observation: recorded as a scan error, chain stays.
    f.scan_at(ts(2_000));
    assert_eq!(f.status(), ChainStatus::Active);
    assert_eq!(f.scan_error().as_deref(), Some("branch 'feat' not found"));

    // Second scan too soon: still protected (mid-rebase window).
    f.scan_at(ts(2_005));
    assert_eq!(f.status(), ChainStatus::Active);

    // ≥ 10s after the *first* missing observation: abandoned.
    f.scan_at(ts(2_011));
    assert_eq!(f.status(), ChainStatus::Abandoned);
    assert_eq!(f.scan_error(), None);
    assert_eq!(f.appended("chain_closed"), 1);
    assert_eq!(keep_ref_count(&f), 0);

    // Further scans with the branch still gone stay quiet.
    let outcome = f.scan_at(ts(2_100));
    assert!(outcome.entries.is_empty());
    assert_eq!(f.status(), ChainStatus::Abandoned);
    assert_eq!(f.appended("chain_closed"), 1);
}

#[test]
fn branch_reappearing_resets_the_missing_window() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan_at(ts(1_000));

    // Mid-rebase: ref vanishes, then comes back.
    f.delete_branch("feat");
    f.scan_at(ts(2_000));
    f.branch("feat", c1);
    f.scan_at(ts(2_004));
    assert_eq!(f.status(), ChainStatus::Active);
    assert_eq!(f.scan_error(), None);

    // Vanishes again: the 10s window restarts from this observation.
    f.delete_branch("feat");
    f.scan_at(ts(3_000));
    f.scan_at(ts(3_005));
    assert_eq!(f.status(), ChainStatus::Active, "window restarted");
    f.scan_at(ts(3_011));
    assert_eq!(f.status(), ChainStatus::Abandoned);
}

#[test]
fn abandoned_chain_reopens_when_branch_returns() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan_at(ts(1_000));
    f.review("I001", "approve");

    f.delete_branch("feat");
    f.scan_at(ts(2_000));
    f.scan_at(ts(2_011));
    assert_eq!(f.status(), ChainStatus::Abandoned);

    f.branch("feat", c1);
    f.scan_at(ts(3_000));
    assert_eq!(f.status(), ChainStatus::Active);
    assert_eq!(
        f.change("I001").status,
        Status::Approved,
        "review state intact"
    );
    assert_eq!(
        f.change("I001").latest_revision().unwrap().number,
        1,
        "same commit: no new revision"
    );
}

#[test]
fn merged_despite_amend_context_drift() {
    let mut f = Fixture::new();
    // Two changes in one file, close enough that amending change one
    // rewrites change two's diff context.
    let b0 = f.commit(&[f.root], "seed\n", &[("f.txt", "a\nb\nc\nd\ne\n")]);
    f.branch("main", b0);
    let c1 = f.commit(&[b0], &msg("one", "I001"), &[("f.txt", "A1\nb\nc\nd\ne\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("f.txt", "A1\nb\nc\nD\ne\n")]);
    f.branch("feat", c2);
    f.scan();

    // The agent amends change one (A1 → A2), rebases change two on top, and
    // ff-merges without an intermediate scan. Both stored diffs now differ
    // from what landed; only the Change-Id trailers still match.
    let c1r = f.commit(&[b0], &msg("one", "I001"), &[("f.txt", "A2\nb\nc\nd\ne\n")]);
    let c2r = f.commit(
        &[c1r],
        &msg("two", "I002"),
        &[("f.txt", "A2\nb\nc\nD\ne\n")],
    );
    f.branch("feat", c2r);
    f.branch("main", c2r);
    f.scan();
    assert_eq!(f.status(), ChainStatus::Merged);
}

#[test]
fn orphaned_chain_still_detects_merge() {
    let mut f = Fixture::new();
    let b0 = f.commit(&[f.root], "seed\n", &[("f.txt", "a\nb\nc\nd\ne\n")]);
    f.branch("main", b0);
    let c1 = f.commit(&[b0], &msg("one", "I001"), &[("f.txt", "A1\nb\nc\nd\ne\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("f.txt", "A1\nb\nc\nD\ne\n")]);
    f.branch("feat", c2);
    f.scan();

    // Agent rebuilds from scratch: reset-to-base must NOT read as merged,
    // and orphans every change.
    f.branch("feat", b0);
    f.scan();
    assert_eq!(f.status(), ChainStatus::Active);
    assert!(f.changes().iter().all(|c| c.orphaned));

    // The work lands on main anyway (rebased elsewhere); even with every
    // change orphaned the trailer quorum must recognize the merge.
    let c1r = f.commit(&[b0], &msg("one", "I001"), &[("f.txt", "A2\nb\nc\nd\ne\n")]);
    let c2r = f.commit(
        &[c1r],
        &msg("two", "I002"),
        &[("f.txt", "A2\nb\nc\nD\ne\n")],
    );
    f.branch("feat", c2r);
    f.branch("main", c2r);
    f.scan();
    assert_eq!(f.status(), ChainStatus::Merged);
}

#[test]
fn orphan_reattach_keeps_approval_carried_across_pure_rebase() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    f.review("I001", "approve");

    // Pure rebase onto a moved main: revision 2, approval carried while the
    // review row stays on revision 1.
    let m1 = f.commit(&[f.root], "main moves\n", &[("m.txt", "m\n")]);
    f.branch("main", m1);
    let c1r = f.commit(&[m1], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1r);
    f.scan();
    assert_eq!(f.change("I001").status, Status::Approved);
    assert_eq!(f.change("I001").latest_revision().unwrap().number, 2);

    // Orphan (rebuild from base) and restore the exact rebased commit: the
    // retained status must come back as approved, not pending.
    f.branch("feat", m1);
    f.scan();
    assert!(f.change("I001").orphaned);
    f.branch("feat", c1r);
    f.scan();
    assert_eq!(
        f.change("I001").status,
        Status::Approved,
        "approval survived"
    );
    assert_eq!(
        f.change("I001").latest_revision().unwrap().number,
        2,
        "no spurious revision"
    );
}
