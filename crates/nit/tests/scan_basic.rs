//! Scan happy path: change creation, revisions, amends, events,
//! idempotency, warnings, and scan-failure isolation.

mod common;

use common::{Fixture, msg};
use nit::db::{self, ChangeStatus};
use nit::gitscan::MERGE_COMMIT_ERROR;

#[test]
fn happy_path_creates_changes_and_revisions() {
    let mut f = Fixture::new();
    let c1 = f.commit(
        &[f.root],
        &msg("server: add health endpoint", "I001"),
        &[("a.rs", "a\n")],
    );
    let c2 = f.commit(
        &[c1],
        &msg("server: add chains api", "I002"),
        &[("b.rs", "b\n")],
    );
    f.branch("feat", c2);

    let outcome = f.scan();
    assert!(outcome.updated);
    assert!(outcome.warnings.is_empty());
    assert_eq!(outcome.chain.last_scan_error, None);

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].change_key, "I001");
    assert_eq!(changes[0].position, Some(0));
    assert_eq!(changes[0].status, ChangeStatus::Pending);
    assert_eq!(changes[1].change_key, "I002");
    assert_eq!(changes[1].position, Some(1));

    let rev1 = f.latest_rev(changes[0].id);
    assert_eq!(rev1.number, 1);
    assert_eq!(rev1.commit_sha, c1.to_string());
    assert_eq!(rev1.parent_sha, f.root.to_string());
    // No fixups: effective tree is the commit's own tree.
    assert_eq!(rev1.effective_tree.as_deref(), Some(f.tree_of(c1).as_str()));
    assert!(rev1.fixups.is_empty());
    assert!(rev1.message.starts_with("server: add health endpoint"));

    let rev2 = f.latest_rev(changes[1].id);
    assert_eq!(rev2.parent_sha, c1.to_string());

    assert_eq!(f.events("chain_updated"), 1);
}

#[test]
fn rescan_without_changes_is_quiet() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    assert!(f.scan().updated);

    let outcome = f.scan();
    assert!(!outcome.updated);
    assert_eq!(
        f.events("chain_updated"),
        1,
        "no event without a structural change"
    );
    let changes = f.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(f.latest_rev(changes[0].id).number, 1);
}

#[test]
fn amend_creates_new_revision_and_resets_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    // Amend: same Change-Id, different content → new revision, pending.
    let c1b = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "different\n")]);
    f.branch("feat", c1b);
    let outcome = f.scan();
    assert!(outcome.updated);

    let changes = f.changes();
    assert_eq!(changes.len(), 1, "same change row across the amend");
    assert_eq!(changes[0].id, change.id);
    assert_eq!(changes[0].status, ChangeStatus::Pending);
    let rev = f.latest_rev(change.id);
    assert_eq!(rev.number, 2);
    assert_eq!(rev.commit_sha, c1b.to_string());
}

#[test]
fn reword_only_amend_keeps_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    // Same diff (patch-id equal), same (empty) fixups — only the message
    // changed: rule 6 treats this as a pure rebase and keeps the status.
    let c1b = f.commit(
        &[f.root],
        &msg("one\n\nnow with body", "I001"),
        &[("a.rs", "a\n")],
    );
    f.branch("feat", c1b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes[0].status, ChangeStatus::Approved);
    assert_eq!(f.latest_rev(change.id).number, 2);
}

#[test]
fn new_commit_appends_change() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.rs", "b\n")]);
    f.branch("feat", c2);
    let outcome = f.scan();
    assert!(outcome.updated);

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1].change_key, "I002");
    assert_eq!(changes[1].position, Some(1));
    assert_eq!(
        f.latest_rev(changes[0].id).number,
        1,
        "untouched change keeps revision 1"
    );
}

#[test]
fn missing_change_id_keys_off_first_sha() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], "no trailer here\n", &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes[0].change_key, c1.to_string());
}

#[test]
fn duplicate_change_id_gets_derived_key_and_warning() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "Idup"), &[("a.rs", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "Idup"), &[("b.rs", "b\n")]);
    f.branch("feat", c2);

    let outcome = f.scan();
    assert_eq!(outcome.warnings.len(), 1);
    assert!(outcome.warnings[0].contains("duplicate Change-Id Idup"));

    let changes = f.changes();
    assert_eq!(changes[0].change_key, "Idup");
    assert_eq!(changes[1].change_key, "Idup#2");

    // Stable across rescans: same derived keys, no new rows.
    let outcome = f.scan();
    assert!(!outcome.updated);
    assert_eq!(
        outcome.warnings.len(),
        1,
        "warning re-surfaces while the duplicate exists"
    );
    assert_eq!(f.changes().len(), 2);
}

#[test]
fn merge_commit_aborts_scan_and_keeps_state() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    // A merge of main into feat poisons the chain.
    let side = f.commit(&[f.root], &msg("side", "I00s"), &[("s.rs", "s\n")]);
    let merge = f.commit(&[c1, side], "Merge main into feat\n", &[]);
    f.branch("feat", merge);

    let outcome = f.scan();
    assert!(!outcome.updated);
    assert_eq!(
        outcome.chain.last_scan_error.as_deref(),
        Some(MERGE_COMMIT_ERROR)
    );

    // Prior state is fully preserved.
    let changes = f.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].status, ChangeStatus::Pending);
    assert_eq!(changes[0].position, Some(0));
    assert_eq!(f.events("chain_updated"), 1);

    // Rebasing away the merge clears the error.
    f.branch("feat", c1);
    let outcome = f.scan();
    assert_eq!(outcome.chain.last_scan_error, None);
}

#[test]
fn unresolvable_base_sets_scan_error() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    let repo_row =
        db::get_or_create_repo(&f.conn, f.repo.path().parent().unwrap().to_str().unwrap()).unwrap();
    let chain = db::get_or_create_chain(&f.conn, repo_row.id, "feat", "no-such-ref").unwrap();
    assert_eq!(chain.id, f.chain_id);

    let outcome = f.scan();
    assert!(
        outcome
            .chain
            .last_scan_error
            .unwrap()
            .contains("no-such-ref")
    );
    assert_eq!(f.changes().len(), 1, "prior state kept");
}
