//! Identity across rewrites: pure rebases, reorders, drop/restore
//! orphaning and re-attachment — the Change-Id trailer is the identity.

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

    // Restore the exact same commit: its trailer re-attaches the orphan
    // and the status returns to its pre-orphan value.
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
fn identity_follows_the_trailer() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    let before = f.changes();

    // Swap the *contents* but keep the trailers: identity follows the
    // Change-Id, no matter how much the diff changed.
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
fn new_change_id_is_a_new_change() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let original = f.changes().remove(0);
    f.review(original.id, "approve");

    // Same diff, same subject, different trailer: gerrit semantics — the
    // Change-Id is the identity, so this is a new change and the old row
    // orphans (its review history stays with the old id).
    let c1b = f.commit(&[f.root], &msg("one", "I00b"), &[("a.txt", "a\n")]);
    f.branch("feat", c1b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    let new = changes.iter().find(|c| c.change_key == "I00b").unwrap();
    assert_eq!(new.position, Some(0));
    assert_eq!(new.status, ChangeStatus::Pending);
    let old = changes.iter().find(|c| c.id == original.id).unwrap();
    assert_eq!(old.status, ChangeStatus::Orphaned);
}
