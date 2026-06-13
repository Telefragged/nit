//! Scan happy path: change creation, revisions, amends, the `revisions`
//! log entry, idempotency, Change-Id validation, and scan-failure isolation.

mod common;

use common::{Fixture, msg};
use nit::db;
use nit::gitscan::{self, MERGE_COMMIT_ERROR};
use nit::review::Status;

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
    assert!(!outcome.entries.is_empty());
    assert_eq!(f.scan_error(), None);

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].change_key, "I001");
    assert_eq!(changes[0].position, Some(0));
    assert_eq!(changes[0].status, Status::Pending);
    assert_eq!(changes[1].change_key, "I002");
    assert_eq!(changes[1].position, Some(1));

    let rev1 = changes[0].latest_revision().unwrap();
    assert_eq!(rev1.number, 1);
    assert_eq!(rev1.commit_sha, c1.to_string());
    assert_eq!(rev1.parent_sha, f.root.to_string());
    assert!(rev1.message.starts_with("server: add health endpoint"));

    assert_eq!(
        changes[1].latest_revision().unwrap().parent_sha,
        c1.to_string()
    );
    assert_eq!(f.appended("revisions"), 1);
}

#[test]
fn rescan_without_changes_is_quiet() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    assert!(!f.scan().entries.is_empty());

    let outcome = f.scan();
    assert!(outcome.entries.is_empty());
    assert_eq!(
        f.appended("revisions"),
        1,
        "no entry without a structural change"
    );
    assert_eq!(f.changes().len(), 1);
    assert_eq!(f.change("I001").latest_revision().unwrap().number, 1);
}

#[test]
fn amend_creates_new_revision_and_resets_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    f.review("I001", "approve");

    // Amend: same Change-Id, different content → new revision, pending.
    let c1b = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "different\n")]);
    f.branch("feat", c1b);
    assert!(!f.scan().entries.is_empty());

    let changes = f.changes();
    assert_eq!(changes.len(), 1, "same change across the amend");
    assert_eq!(changes[0].status, Status::Pending);
    let rev = changes[0].latest_revision().unwrap();
    assert_eq!(rev.number, 2);
    assert_eq!(rev.commit_sha, c1b.to_string());
}

#[test]
fn reword_only_amend_resets_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    f.review("I001", "approve");

    // Same diff (patch-id equal) — only the message changed. A reword is
    // reviewable (/COMMIT_MSG), so it is not a pure rebase: status resets.
    let c1b = f.commit(
        &[f.root],
        &msg("one\n\nnow with body", "I001"),
        &[("a.rs", "a\n")],
    );
    f.branch("feat", c1b);
    f.scan();

    assert_eq!(f.change("I001").status, Status::Pending);
    assert_eq!(f.change("I001").latest_revision().unwrap().number, 2);
}

#[test]
fn new_commit_appends_change() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("b.rs", "b\n")]);
    f.branch("feat", c2);
    assert!(!f.scan().entries.is_empty());

    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1].change_key, "I002");
    assert_eq!(changes[1].position, Some(1));
    assert_eq!(
        changes[0].latest_revision().unwrap().number,
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
    assert!(f.scan().entries.is_empty());
    let err = f.scan_error().unwrap();
    assert!(err.contains("without a Change-Id trailer"), "{err}");
    assert!(err.contains(&c2.to_string()[..12]), "{err}");
    assert_eq!(f.changes().len(), 1, "prior state kept");

    // Adding the trailer (new sha) clears the error.
    let c2b = f.commit(&[c1], &msg("two", "I002"), &[("b.rs", "b\n")]);
    f.branch("feat", c2b);
    f.scan();
    assert_eq!(f.scan_error(), None);
    assert_eq!(f.changes().len(), 2);
}

#[test]
fn duplicate_change_id_fails_scan() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "Idup"), &[("a.rs", "a\n")]);
    let c2 = f.commit(&[c1], &msg("two", "Idup"), &[("b.rs", "b\n")]);
    f.branch("feat", c2);

    f.scan();
    let err = f.scan_error().unwrap();
    assert!(err.contains("duplicate Change-Id Idup"), "{err}");
    assert_eq!(f.changes().len(), 0, "nothing reconciled");
}

#[test]
fn fixup_commit_fails_scan() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    let fx = f.commit(&[c1], "fixup! one\n", &[("a.rs", "a2\n")]);
    f.branch("feat", fx);
    f.scan();
    let err = f.scan_error().unwrap();
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

    let side = f.commit(&[f.root], &msg("side", "I00s"), &[("s.rs", "s\n")]);
    let merge = f.commit(&[c1, side], "Merge main into feat\n", &[]);
    f.branch("feat", merge);

    assert!(f.scan().entries.is_empty());
    assert_eq!(f.scan_error().as_deref(), Some(MERGE_COMMIT_ERROR));

    let changes = f.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].status, Status::Pending);
    assert_eq!(changes[0].position, Some(0));
    assert_eq!(f.appended("revisions"), 1);

    // Rebasing away the merge clears the error.
    f.branch("feat", c1);
    f.scan();
    assert_eq!(f.scan_error(), None);
}

#[test]
fn register_validates_resolvable_refs() {
    let f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    let workdir = f.repo.workdir().unwrap();

    // Unresolvable branch/base/repo are registration-time errors (HTTP 400).
    assert!(gitscan::validate_registration(workdir, "nope", "main").is_err());
    assert!(gitscan::validate_registration(workdir, "feat", "nope").is_err());
    assert!(
        gitscan::validate_registration(std::path::Path::new("/no/such/dir"), "feat", "main")
            .is_err()
    );
    assert!(gitscan::validate_registration(workdir, "feat", "main").is_ok());
}

#[test]
fn chain_registration_is_idempotent_and_moves_base() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("nit.sqlite3")).unwrap();
    let a = db::get_or_create_chain(&conn, "/repo", "feat", "main").unwrap();
    let b = db::get_or_create_chain(&conn, "/repo", "feat", "main").unwrap();
    assert_eq!(a.id, b.id, "idempotent re-registration");
    let moved = db::get_or_create_chain(&conn, "/repo", "feat", "HEAD").unwrap();
    assert_eq!(moved.id, a.id);
    assert_eq!(moved.base, "HEAD");
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
    f.scan();
    assert!(f.scan_error().unwrap().contains("root commit"));
    assert_eq!(f.changes().len(), 1, "prior state kept");
}

#[test]
fn unresolvable_base_sets_scan_error() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    f.branch("feat", c1);
    f.scan();

    f.proj.base = "no-such-ref".to_string();
    f.scan();
    assert!(f.scan_error().unwrap().contains("no-such-ref"));
    assert_eq!(f.changes().len(), 1, "prior state kept");
}
