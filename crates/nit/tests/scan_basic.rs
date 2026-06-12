//! Scan happy path: change creation, revisions, amends, events,
//! idempotency, Change-Id validation, and scan-failure isolation.

mod common;

use common::{Fixture, msg};
use nit::db::{self, ChangeStatus};
use nit::gitscan::{self, MERGE_COMMIT_ERROR};

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
fn reword_only_amend_resets_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    // Same diff (patch-id equal) — only the message changed. The message
    // is reviewable (/COMMIT_MSG), so rule 4 does not treat a reword as a
    // pure rebase: the reviewer must look again.
    let c1b = f.commit(
        &[f.root],
        &msg("one\n\nnow with body", "I001"),
        &[("a.rs", "a\n")],
    );
    f.branch("feat", c1b);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes[0].status, ChangeStatus::Pending);
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
fn missing_change_id_fails_scan_and_keeps_state() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    // A new commit without a trailer poisons the chain until it is fixed.
    let c2 = f.commit(&[c1], "no trailer here\n", &[("b.rs", "b\n")]);
    f.branch("feat", c2);
    let outcome = f.scan();
    assert!(!outcome.updated);
    let err = outcome.chain.last_scan_error.unwrap();
    assert!(err.contains("without a Change-Id trailer"), "{err}");
    assert!(err.contains(&c2.to_string()[..12]), "{err}");
    assert_eq!(f.changes().len(), 1, "prior state kept");

    // Adding the trailer (new sha) clears the error.
    let c2b = f.commit(&[c1], &msg("two", "I002"), &[("b.rs", "b\n")]);
    f.branch("feat", c2b);
    let outcome = f.scan();
    assert_eq!(outcome.chain.last_scan_error, None);
    assert_eq!(f.changes().len(), 2);
}

#[test]
fn duplicate_change_id_fails_scan() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "Idup"), &[("a.rs", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "Idup"), &[("b.rs", "b\n")]);
    f.branch("feat", c2);

    let outcome = f.scan();
    let err = outcome.chain.last_scan_error.unwrap();
    assert!(err.contains("duplicate Change-Id Idup"), "{err}");
    assert_eq!(f.changes().len(), 0, "nothing reconciled");
}

#[test]
fn fixup_commit_fails_scan() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    // fixup!/squash! commits are a local pre-push convenience only:
    // the agent must autosquash before pushing.
    let fx = f.commit(&[c1], "fixup! one\n", &[("a.rs", "a2\n")]);
    f.branch("feat", fx);
    let outcome = f.scan();
    let err = outcome.chain.last_scan_error.unwrap();
    assert!(err.contains("fixup!/squash!"), "{err}");
    assert!(err.contains(&fx.to_string()[..12]), "{err}");
    assert_eq!(f.changes().len(), 1, "prior state kept");
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
fn register_validates_and_canonicalizes() {
    let f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    let workdir = f.repo.path().parent().unwrap();

    // Unresolvable branch/base are registration-time errors (HTTP 400).
    assert!(gitscan::register(&f.conn, workdir, "nope", "main").is_err());
    assert!(gitscan::register(&f.conn, workdir, "feat", "nope").is_err());
    assert!(
        gitscan::register(
            &f.conn,
            std::path::Path::new("/no/such/dir"),
            "feat",
            "main"
        )
        .is_err()
    );

    // A non-canonical spelling of the path lands on the same chain row.
    let chain_a = gitscan::register(&f.conn, workdir, "feat", "main").unwrap();
    let chain_b = gitscan::register(&f.conn, &workdir.join("."), "feat", "main").unwrap();
    assert_eq!(chain_a.id, chain_b.id, "idempotent re-registration");

    // Re-registration can move the base.
    let with_base = gitscan::register(&f.conn, workdir, "feat", "HEAD").unwrap();
    assert_eq!(with_base.id, chain_a.id);
    assert_eq!(with_base.base, "HEAD");
}

#[test]
fn unrelated_root_commit_sets_scan_error() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    // A branch rebuilt from an unrelated root: the walk hits a parentless
    // commit, which the diff/identity model cannot represent.
    let rogue = f.commit(&[], "unrelated root\n", &[("z.rs", "z\n")]);
    f.branch("feat", rogue);
    let outcome = f.scan();
    assert!(
        outcome
            .chain
            .last_scan_error
            .unwrap()
            .contains("root commit")
    );
    assert_eq!(f.changes().len(), 1, "prior state kept");
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
