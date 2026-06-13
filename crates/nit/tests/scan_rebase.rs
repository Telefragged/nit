//! Identity across rewrites: pure rebases, reorders, drop/restore
//! orphaning and re-attachment — the Change-Id trailer is the identity.

mod common;

use common::{Fixture, msg};
use nit::review::Status;

#[test]
fn pure_rebase_keeps_status_and_positions() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    f.review("I001", "approve");
    f.review("I002", "request_changes");

    // main moves on; the agent rebases (same diffs, new shas, new parents).
    let m1 = f.commit(&[f.root], "main: unrelated\n", &[("main.txt", "m\n")]);
    f.branch("main", m1);
    let c1b = f.commit(&[m1], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2b = f.commit(&[c1b], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2b);

    assert!(!f.scan().entries.is_empty());
    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(
        changes[0].status,
        Status::Approved,
        "pure rebase keeps status"
    );
    assert_eq!(changes[1].status, Status::ChangesRequested);
    let rev = changes[0].latest_revision().unwrap();
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
    f.review("I001", "approve");
    f.review("I002", "request_changes");

    // Swap the two commits (independent files: diffs unchanged).
    let c2b = f.commit(&[f.root], &msg("two", "I002"), &[("b.txt", "b\n")]);
    let c1b = f.commit(&[c2b], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1b);

    f.scan();
    let changes = f.changes(); // ordered by position
    assert_eq!(changes[0].change_key, "I002");
    assert_eq!(changes[0].position, Some(0));
    assert_eq!(changes[0].status, Status::ChangesRequested);
    assert_eq!(changes[1].change_key, "I001");
    assert_eq!(changes[1].position, Some(1));
    assert_eq!(changes[1].status, Status::Approved);
    assert_eq!(changes[1].latest_revision().unwrap().number, 2);
}

#[test]
fn drop_and_restore_reattaches_with_previous_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    f.branch("feat", c2);
    f.scan();
    f.review("I002", "approve");

    // Drop the second commit.
    f.branch("feat", c1);
    assert!(!f.scan().entries.is_empty());
    let changes = f.changes();
    assert_eq!(changes.len(), 2, "changes are never deleted");
    let orphan = f.change("I002");
    assert!(orphan.orphaned);
    assert_eq!(orphan.position, None);

    // Restore the exact same commit: its trailer re-attaches the orphan and
    // the status returns to its pre-orphan value.
    f.branch("feat", c2);
    f.scan();
    let restored = f.change("I002");
    assert_eq!(restored.status, Status::Approved);
    assert_eq!(restored.position, Some(1));
    assert_eq!(
        restored.latest_revision().unwrap().number,
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
    let id001 = f.change("I001").id;

    // Swap the *contents* but keep the trailers: identity follows the
    // Change-Id, no matter how much the diff changed.
    let c1b = f.commit(&[f.root], &msg("one", "I001"), &[("b.txt", "b\n")]);
    let c2b = f.commit(&[c1b], &msg("two", "I002"), &[("a.txt", "a\n")]);
    f.branch("feat", c2b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes[0].id, id001);
    assert_eq!(changes[0].change_key, "I001");
    assert_eq!(
        changes[0].latest_revision().unwrap().commit_sha,
        c1b.to_string()
    );
    assert_eq!(changes[1].change_key, "I002");
    assert_eq!(
        changes[1].latest_revision().unwrap().commit_sha,
        c2b.to_string()
    );
}

#[test]
fn new_change_id_is_a_new_change() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let id001 = f.change("I001").id;
    f.review("I001", "approve");

    // Same diff, same subject, different trailer: gerrit semantics — the
    // Change-Id is the identity, so this is a new change and the old change
    // orphans (its review history stays with the old id).
    let c1b = f.commit(&[f.root], &msg("one", "I00b"), &[("a.txt", "a\n")]);
    f.branch("feat", c1b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    let new = f.change("I00b");
    assert_eq!(new.position, Some(0));
    assert_eq!(new.status, Status::Pending);
    let old = f.change("I001");
    assert_eq!(old.id, id001);
    assert!(old.orphaned);
}
