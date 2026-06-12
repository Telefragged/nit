//! Identity across rewrites: pure rebases, reorders, drop/restore
//! orphaning and re-attachment, subject vs patch-id vs trailer matching.

mod common;

use common::{Fixture, msg};
use nit::db::ChangeStatus;

#[test]
fn pure_rebase_keeps_status_and_positions() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    let changes = f.changes();
    f.review(changes[0].id, "approve");
    f.review(changes[1].id, "request_changes");

    // main moves on; the agent rebases (same diffs, new shas, new parents).
    let m1 = f.commit(&[f.root], "main: unrelated\n", &[("main.txt", "m\n")]);
    f.branch("main", m1);
    let c1b = f.commit(&[m1], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2b = f.commit(&[c1b], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2b);

    let outcome = f.scan();
    assert!(outcome.updated);
    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(
        changes[0].status,
        ChangeStatus::Approved,
        "pure rebase keeps status"
    );
    assert_eq!(changes[1].status, ChangeStatus::ChangesRequested);
    let rev = f.latest_rev(changes[0].id);
    assert_eq!(rev.number, 2);
    assert_eq!(rev.commit_sha, c1b.to_string());
    assert_eq!(rev.parent_sha, m1.to_string());
}

#[test]
fn rebase_preserving_fixups_is_still_pure() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let f1 = f.commit(&[c1], "fixup! one\n", &[("b.txt", "b\n")]);
    f.branch("feat", f1);
    f.scan();
    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    let m1 = f.commit(&[f.root], "main: unrelated\n", &[("main.txt", "m\n")]);
    f.branch("main", m1);
    let c1b = f.commit(&[m1], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let f1b = f.commit(&[c1b], "fixup! one\n", &[("b.txt", "b\n")]);
    f.branch("feat", f1b);

    f.scan();
    let changes = f.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].status,
        ChangeStatus::Approved,
        "rebased fixup is patch-id-equal: pure rebase"
    );
    let rev = f.latest_rev(change.id);
    assert_eq!(rev.number, 2);
    assert_eq!(rev.fixups[0].sha, f1b.to_string());
}

#[test]
fn reorder_updates_positions_and_keeps_statuses() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    let before = f.changes();
    f.review(before[0].id, "approve");
    f.review(before[1].id, "request_changes");

    // Swap the two commits (independent files: diffs unchanged).
    let c2b = f.commit(&[f.root], &msg("two", "I002"), &[("b.txt", "b\n")]);
    let c1b = f.commit(&[c2b], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1b);

    f.scan();
    let changes = f.changes(); // ordered by position
    assert_eq!(changes[0].change_key, "I002");
    assert_eq!(changes[0].position, Some(0));
    assert_eq!(changes[0].status, ChangeStatus::ChangesRequested);
    assert_eq!(changes[1].change_key, "I001");
    assert_eq!(changes[1].position, Some(1));
    assert_eq!(changes[1].status, ChangeStatus::Approved);
    assert_eq!(f.latest_rev(changes[1].id).number, 2);
}

#[test]
fn drop_and_restore_reattaches_with_previous_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    let two = f.changes().remove(1);
    f.review(two.id, "approve");

    // Drop the second commit.
    f.branch("feat", c1);
    let outcome = f.scan();
    assert!(outcome.updated);
    let changes = f.changes();
    assert_eq!(changes.len(), 2, "rows are never deleted");
    let orphan = changes.iter().find(|c| c.id == two.id).unwrap();
    assert_eq!(orphan.status, ChangeStatus::Orphaned);
    assert_eq!(orphan.position, None);

    // Restore the exact same commit: rule 2 re-attaches the orphan and
    // the status returns to its pre-orphan value.
    f.branch("feat", c2);
    f.scan();
    let restored = f.changes().into_iter().find(|c| c.id == two.id).unwrap();
    assert_eq!(restored.status, ChangeStatus::Approved);
    assert_eq!(restored.position, Some(1));
    assert_eq!(
        f.latest_rev(two.id).number,
        1,
        "same effective state: no new revision"
    );
}

#[test]
fn orphans_do_not_subject_match() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], "refactor parser\n", &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let original = f.changes().remove(0);

    // Drop it; the row orphans.
    f.branch("feat", f.root);
    f.scan();

    // A *different* commit reusing the subject (new diff, no trailer) is a
    // new change: rule 4 only matches changes that were live at scan start.
    let c1b = f.commit(&[f.root], "refactor parser\n", &[("a.txt", "rewritten\n")]);
    f.branch("feat", c1b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    let old = changes.iter().find(|c| c.id == original.id).unwrap();
    assert_eq!(old.status, ChangeStatus::Orphaned, "orphan stays orphaned");
    assert!(
        changes
            .iter()
            .any(|c| c.id != original.id && c.position == Some(0))
    );
}

#[test]
fn subject_match_when_commit_left_the_branch() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], "refactor parser\n", &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    // Rewritten in place (same scan sees the old commit gone): new sha,
    // new diff, same subject → rule 4 keeps the identity.
    let c1b = f.commit(&[f.root], "refactor parser\n", &[("a.txt", "v2\n")]);
    f.branch("feat", c1b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes.len(), 1, "same change row");
    assert_eq!(changes[0].id, change.id);
    assert_eq!(
        changes[0].status,
        ChangeStatus::Pending,
        "diff changed: re-review"
    );
    assert_eq!(f.latest_rev(change.id).number, 2);
}

#[test]
fn patch_id_match_survives_subject_rewrite() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], "old subject\n", &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    // Same diff, new sha and new subject → rule 3 (patch-id) keeps the
    // identity, but the message changed (reviewable as /COMMIT_MSG), so
    // it is not a pure rebase: the reviewer must look again.
    let c1b = f.commit(&[f.root], "new subject\n", &[("a.txt", "a\n")]);
    f.branch("feat", c1b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id, change.id);
    assert_eq!(changes[0].status, ChangeStatus::Pending);
    assert_eq!(f.latest_rev(change.id).number, 2);
}

#[test]
fn trailer_beats_patch_id() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    let before = f.changes();

    // Swap the *contents* but keep the trailers: identity follows the
    // Change-Id (rule 1 outranks patch-id).
    let c1b = f.commit(&[f.root], &msg("one", "I001"), &[("b.txt", "b\n")]);
    let c2b = f.commit(&[c1b], &msg("two", "I002"), &[("a.txt", "a\n")]);
    f.branch("feat", c2b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes[0].id, before[0].id);
    assert_eq!(changes[0].change_key, "I001");
    assert_eq!(f.latest_rev(changes[0].id).commit_sha, c1b.to_string());
    assert_eq!(changes[1].change_key, "I002");
    assert_eq!(f.latest_rev(changes[1].id).commit_sha, c2b.to_string());
}

#[test]
fn duplicate_subjects_rebase_keeps_identities_by_patch_id() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], "dup\n", &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], "dup\n", &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    let before = f.changes();
    f.review(before[0].id, "approve");

    let m1 = f.commit(&[f.root], "main: unrelated\n", &[("main.txt", "m\n")]);
    f.branch("main", m1);
    let c1b = f.commit(&[m1], "dup\n", &[("a.txt", "a\n")]);
    let c2b = f.commit(&[c1b], "dup\n", &[("b.txt", "b\n")]);
    f.branch("feat", c2b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes.len(), 2, "no new rows despite identical subjects");
    assert_eq!(changes[0].id, before[0].id);
    assert_eq!(changes[0].status, ChangeStatus::Approved);
    assert_eq!(f.latest_rev(changes[0].id).commit_sha, c1b.to_string());
    assert_eq!(f.latest_rev(changes[1].id).commit_sha, c2b.to_string());
}
