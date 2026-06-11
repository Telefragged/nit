//! Fixup folding: effective trees, conflicts, fixup-of-fixup, squash
//! warnings, keep refs and re-fold repair.

mod common;

use common::{Fixture, msg};
use git2::Oid;
use nit::db::{self, ChangeStatus};

#[test]
fn fixup_folds_into_new_revision() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "one\ntwo\n")]);
    f.branch("feat", c1);
    f.scan();
    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    let f1 = f.commit(&[c1], "fixup! one\n", &[("b.txt", "from fixup\n")]);
    f.branch("feat", f1);
    let outcome = f.scan();
    assert!(outcome.updated);

    let changes = f.changes();
    assert_eq!(changes.len(), 1, "fixup is not its own change");
    assert_eq!(
        changes[0].status,
        ChangeStatus::Pending,
        "new fixup means the reviewer must look again"
    );

    let rev = f.latest_rev(change.id);
    assert_eq!(rev.number, 2);
    assert_eq!(
        rev.commit_sha,
        c1.to_string(),
        "revision keeps the target commit"
    );
    assert_eq!(rev.fixups.len(), 1);
    assert_eq!(rev.fixups[0].sha, f1.to_string());
    assert!(rev.fixups[0].message.starts_with("fixup! one"));

    let eff = rev.effective_tree.unwrap();
    assert_ne!(eff, f.tree_of(c1), "fold produced a new tree");
    assert_eq!(f.blob_in_tree(&eff, "a.txt"), "one\ntwo\n");
    assert_eq!(f.blob_in_tree(&eff, "b.txt"), "from fixup\n");

    // GC safety: keep ref pins parent, original and fold.
    let keep = f
        .repo
        .find_reference(&format!("refs/nit/keep/{}/{}/2", f.chain_id, change.id))
        .expect("keep ref exists");
    let synthetic = f.repo.find_commit(keep.target().unwrap()).unwrap();
    assert_eq!(synthetic.tree_id().to_string(), eff);
    assert_eq!(synthetic.parent_id(0).unwrap(), f.root);
    assert_eq!(synthetic.parent_id(1).unwrap(), c1);
}

#[test]
fn fixup_fold_conflict_sets_needs_rebase() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("f.txt", "one\n")]);
    let c2 = f.commit(&[c1], &msg("two", "I002"), &[("f.txt", "two\n")]);
    // The fixup targets c1 but edits on top of c2's content: ancestor
    // (its parent tree, f.txt="two") vs ours (c1, "one") vs theirs
    // ("three") — both sides changed the same line: conflict.
    let f1 = f.commit(&[c2], "fixup! one\n", &[("f.txt", "three\n")]);
    f.branch("feat", f1);

    f.scan();
    let changes = f.changes();
    assert_eq!(changes.len(), 2);

    let rev1 = f.latest_rev(changes[0].id);
    assert_eq!(rev1.fixups.len(), 1);
    assert_eq!(rev1.effective_tree, None, "conflict -> NULL effective tree");

    // The untargeted change folds normally.
    let rev2 = f.latest_rev(changes[1].id);
    assert!(rev2.effective_tree.is_some());
    assert!(rev2.fixups.is_empty());

    // The keep ref still pins parent+original via the commit's own tree.
    let keep = f
        .repo
        .find_reference(&format!("refs/nit/keep/{}/{}/1", f.chain_id, changes[0].id))
        .expect("keep ref exists despite conflict");
    let synthetic = f.repo.find_commit(keep.target().unwrap()).unwrap();
    assert_eq!(
        synthetic.tree_id(),
        f.repo.find_commit(c1).unwrap().tree_id()
    );
}

#[test]
fn fixup_of_fixup_folds_in_branch_order() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let f1 = f.commit(&[c1], "fixup! one\n", &[("b.txt", "b\n")]);
    let f2 = f.commit(&[f1], "fixup! fixup! one\n", &[("c.txt", "c\n")]);
    f.branch("feat", f2);

    f.scan();
    let changes = f.changes();
    assert_eq!(changes.len(), 1, "fixup-of-fixup chains to the root change");
    let rev = f.latest_rev(changes[0].id);
    assert_eq!(rev.number, 1);
    assert_eq!(
        rev.fixups.iter().map(|x| x.sha.clone()).collect::<Vec<_>>(),
        vec![f1.to_string(), f2.to_string()],
        "branch order"
    );
    let eff = rev.effective_tree.unwrap();
    assert_eq!(f.blob_in_tree(&eff, "b.txt"), "b\n");
    assert_eq!(f.blob_in_tree(&eff, "c.txt"), "c\n");
}

#[test]
fn sequential_fixups_fold_on_each_other() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let f1 = f.commit(&[c1], "fixup! one\n", &[("x.txt", "fa\n")]);
    let f2 = f.commit(&[f1], "fixup! one\n", &[("x.txt", "fb\n")]);
    f.branch("feat", f2);

    f.scan();
    let rev = f.latest_rev(f.changes()[0].id);
    let eff = rev.effective_tree.unwrap();
    assert_eq!(f.blob_in_tree(&eff, "x.txt"), "fb\n", "later fixup wins");
}

#[test]
fn fixup_without_target_is_a_regular_change() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = f.commit(&[c1], "fixup! vanished subject\n", &[("b.txt", "b\n")]);
    f.branch("feat", c2);

    f.scan();
    let changes = f.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1].position, Some(1));
    assert_eq!(changes[1].change_key, c2.to_string());
    assert!(f.latest_rev(changes[1].id).fixups.is_empty());
}

#[test]
fn fixup_attaches_by_sha_prefix() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let short = &c1.to_string()[..7];
    let f1 = f.commit(&[c1], &format!("fixup! {short}\n"), &[("b.txt", "b\n")]);
    f.branch("feat", f1);

    f.scan();
    let changes = f.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(f.latest_rev(changes[0].id).fixups[0].sha, f1.to_string());
}

#[test]
fn squash_folds_with_warning() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let s1 = f.commit(&[c1], "squash! one\n\nextra message\n", &[("b.txt", "b\n")]);
    f.branch("feat", s1);

    let outcome = f.scan();
    assert_eq!(outcome.warnings.len(), 1);
    assert!(outcome.warnings[0].contains("squash!"));
    let changes = f.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(f.latest_rev(changes[0].id).fixups[0].sha, s1.to_string());
}

#[test]
fn empty_fixup_still_resets_status() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    f.branch("feat", c1);
    f.scan();
    let change = f.changes().remove(0);
    f.review(change.id, "approve");

    // An empty fixup (the agent arguing in the message): commit patch-id
    // unchanged, but the fixup list grew → pending, not pure rebase.
    let f1 = f.commit(&[c1], "fixup! one\n\nI disagree, see body.\n", &[]);
    f.branch("feat", f1);
    f.scan();

    let changes = f.changes();
    assert_eq!(changes[0].status, ChangeStatus::Pending);
    let rev = f.latest_rev(change.id);
    assert_eq!(rev.number, 2);
    assert_eq!(rev.effective_tree.as_deref(), Some(f.tree_of(c1).as_str()));
}

#[test]
fn missing_effective_tree_is_refolded() {
    let mut f = Fixture::new();
    let c1 = f.commit(&[f.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let f1 = f.commit(&[c1], "fixup! one\n", &[("b.txt", "b\n")]);
    f.branch("feat", f1);
    f.scan();

    let change = f.changes().remove(0);
    let rev = f.latest_rev(change.id);
    let real_tree = rev.effective_tree.clone().unwrap();

    // Simulate a pruned tree: point the row at an object that's gone.
    let bogus = "deadbeef".repeat(5);
    assert!(f.repo.find_tree(Oid::from_str(&bogus).unwrap()).is_err());
    db::revision_set_effective_tree(&f.conn, rev.id, Some(&bogus)).unwrap();

    let outcome = f.scan();
    assert!(!outcome.updated, "repair is not a structural change");
    let rev = f.latest_rev(change.id);
    assert_eq!(rev.effective_tree.as_deref(), Some(real_tree.as_str()));
}
