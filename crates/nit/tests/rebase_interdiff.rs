//! Rebase-aware interdiffs: the problem reproduced and each observable
//! property checked against real git.
//!
//! Most tests drive the diff machinery directly — an unfiltered `render` is
//! the plain interdiff (what leaked the base movement before), `analyze` plus
//! its `skip`/`tag` is the containment — over four commits standing in for
//! parent(m)/m/parent(n)/n. `analyze` reads only the shas it is given, so the
//! four commits need no real parent/child wiring; their *trees* are what
//! matter. One end-to-end test exercises the HTTP handler so the wiring
//! (threading parent(m), invoking the analysis) is covered too — built by
//! pushing a stack and amending an earlier change so a later one is rewritten
//! by rebase only.

mod common;

use common::*;
use git2::Oid;
use nit::api::diff::{COMMIT_MSG_PATH, commit_tree, git_diff, render};
use nit::api::rebase::{Rev, analyze};
use nit_types::diff::{Diff, DiffFile};
use nit_types::domain::Sha;
use nit_types::domain::{FileStatus, LineKind};

fn body<S: std::borrow::Borrow<str>>(lines: &[S]) -> Vec<u8> {
    let mut s = lines.join("\n");
    s.push('\n');
    s.into_bytes()
}

/// `n` numbered lines — long enough for rename detection to fire on a move.
fn numbered(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("line {i}")).collect()
}

/// `lines` with line `n` rewritten, numbered as a diff numbers them (from 1).
fn edit(lines: &[String], n: usize, text: &str) -> Vec<String> {
    let mut out = lines.to_vec();
    out[n - 1] = text.into();
    out
}

/// A commit whose tree is the repo root plus `files` (root carries only an
/// untouched README, identical across every commit so it never diffs).
fn snapshot(g: &GitRepo, files: &[(&str, &[u8])]) -> Oid {
    g.commit_full(&[g.root], "snapshot\n", files, &[])
}

/// The plain interdiff `tree(m) → tree(n)` and the rebase-aware one, so a test
/// can compare what leaked vs. what is contained. The second follows the
/// endpoint exactly: analyse first, render only the files the drift leaves
/// standing, then tag them.
fn interdiff(g: &GitRepo, m: Oid, parent_m: Oid, n: Oid, parent_n: Oid) -> (Diff, Diff) {
    let sha = |o: Oid| Sha::from(o.to_string());
    let (m, parent_m, n, parent_n) = (sha(m), sha(parent_m), sha(n), sha(parent_n));
    let tm = commit_tree(&g.repo, &m).expect("m tree resolves");
    let tn = commit_tree(&g.repo, &n).expect("n tree resolves");
    let git = git_diff(&g.repo, &tm, &tn).expect("interdiff builds");
    let plain = render(&g.repo, &git, 3, |_| true).expect("plain interdiff renders");
    let drift = analyze(
        &g.repo,
        &git,
        &Rev {
            commit: &m,
            parent: &parent_m,
        },
        &Rev {
            commit: &n,
            parent: &parent_n,
        },
        None,
    )
    .expect("drift analysis succeeds");
    let mut tagged =
        render(&g.repo, &git, 3, |p| drift.renders(p)).expect("contained interdiff renders");
    drift.tag(&mut tagged);
    (plain, tagged)
}

fn file<'a>(diff: &'a Diff, path: &str) -> Option<&'a DiffFile> {
    diff.files.iter().find(|f| f.path == path)
}

fn drift_lines(f: &DiffFile) -> Vec<(char, u64)> {
    let mut out = Vec::new();
    for hunk in &f.hunks {
        for l in &hunk.lines {
            if l.drift {
                if l.kind == LineKind::Add {
                    out.push(('+', l.new.expect("add line has new no")));
                } else if l.kind == LineKind::Del {
                    out.push(('-', l.old.expect("del line has old no")));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Properties 3 & 5.

/// The motivating case: an earlier change C1 is amended, which rewrites the
/// later change C2 onto a new parent. C2's own delta is unchanged (a pure
/// rebase), so C2's r0→r1 interdiff should show *nothing* — but the plain
/// diff leaks C1's amendment as if C2 had made it.
#[test]
fn stacked_amend_leaks_into_a_later_change_until_contained() {
    let g = GitRepo::new();
    let base = ["fn one() {}", "fn two() {}", "fn three() {}"];
    let drifted = ["fn one() {}", "fn TWO() {}", "fn three() {}"];
    let feat = body(&["pub const C2: u8 = 1;"]);

    // parent(m)=C1@r0, m=C2@r0; parent(n)=C1@r1 (amended), n=C2@r1 (rebased).
    let parent_m = snapshot(&g, &[("base.rs", &body(&base))]);
    let m = snapshot(&g, &[("base.rs", &body(&base)), ("feat.rs", &feat)]);
    let parent_n = snapshot(&g, &[("base.rs", &body(&drifted))]);
    let n = snapshot(&g, &[("base.rs", &body(&drifted)), ("feat.rs", &feat)]);

    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);

    // The problem: the plain interdiff shows base.rs changing, work C2 never
    // did — the reviewer would see (and might re-review) C1's amendment.
    let leaked = file(&plain, "base.rs").expect("plain interdiff leaks base.rs");
    assert_eq!((leaked.additions, leaked.deletions), (1, 1));

    // Contained: base.rs is entirely drift, so it drops out; feat.rs is
    // unchanged between r0 and r1 so it was never in the diff. Nothing left.
    assert!(
        tagged.files.is_empty(),
        "a pure rebase collapses to no code files, got {:?}",
        tagged.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Property 1 — no-rebase equivalence (same parent ⇒ byte-identical).

#[test]
fn same_parent_interdiff_is_unchanged() {
    let g = GitRepo::new();
    let parent = snapshot(&g, &[("a.rs", &body(&["one", "two", "three"]))]);
    let m = snapshot(&g, &[("a.rs", &body(&["one", "TWO", "three"]))]);
    let n = snapshot(&g, &[("a.rs", &body(&["one", "TWO!", "three"]))]);

    // Both revisions share `parent`: no drift work, plain == tagged.
    let (plain, tagged) = interdiff(&g, m, parent, n, parent);
    assert_eq!(
        serde_json::to_value(&plain).expect("plain serializes"),
        serde_json::to_value(&tagged).expect("tagged serializes"),
        "same-parent interdiff must be byte-for-byte the plain diff"
    );
}

// ---------------------------------------------------------------------------
// Property 4/6/7.

#[test]
fn mixed_hunk_keeps_real_edit_and_tags_drift() {
    let g = GitRepo::new();
    // base.rs lines 2 (B) and 4 (D) are near enough to share one hunk.
    let parent_m = snapshot(&g, &[("base.rs", &body(&["A", "B", "C", "D", "E"]))]);
    let m = snapshot(&g, &[("base.rs", &body(&["A", "Bm", "C", "D", "E"]))]);
    let parent_n = snapshot(&g, &[("base.rs", &body(&["A", "B", "C", "Dx", "E"]))]);
    let n = snapshot(&g, &[("base.rs", &body(&["A", "Bn", "C", "Dx", "E"]))]); // C2 re-edits B

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "base.rs").expect("mixed file stays in the list");

    // Real edit (B at line 2) counted; drift (D at line 4) tagged, uncounted.
    // The exact drift_lines + (1,1) counts together pin line 4 as drift-only.
    assert_eq!((f.additions, f.deletions), (1, 1));
    assert_eq!(drift_lines(f), vec![('-', 4), ('+', 4)]);
}

// ---------------------------------------------------------------------------
// Regression: duplicate lines let the parent→parent diff fold the change's
// own edit into a base-movement hunk. The drifted lines beside it must still
// be tagged and excluded from the count — not mis-attributed to the change.

#[test]
fn drift_kept_when_the_diff_folds_it_against_a_duplicate_line() {
    let g = GitRepo::new();
    // Duplicate `a`/`b` runs are what make Myers bundle an unchanged line.
    let parent_m = snapshot(
        &g,
        &[("f.rs", &body(&["a", "a", "a", "b", "b", "a", "b", "a"]))],
    );
    // The change edits old line 3 at m.
    let m = snapshot(
        &g,
        &[("f.rs", &body(&["a", "a", "RM", "b", "b", "a", "b", "a"]))],
    );
    // The base drifts line 1 and lines 4,5.
    let parent_n = snapshot(
        &g,
        &[("f.rs", &body(&["DA", "a", "a", "D4", "D5", "a", "b", "a"]))],
    );
    // The change edits line 1 at n and carries the base drift.
    let n = snapshot(
        &g,
        &[("f.rs", &body(&["RN", "a", "a", "D4", "D5", "a", "b", "a"]))],
    );

    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    // The problem: the plain interdiff counts the base movement as the change's.
    assert_eq!(
        {
            let f = file(&plain, "f.rs").expect("plain f.rs");
            (f.additions, f.deletions)
        },
        (3, 3)
    );

    // Contained: only the change's two real edits (RN at new 1, RM removed at
    // old 3) count; the base movement (D4,D5 at new 4,5 / old b,b at 4,5) is
    // tagged drift and excluded.
    let f = file(&tagged, "f.rs").expect("tagged f.rs");
    assert_eq!((f.additions, f.deletions), (1, 1));
    assert_eq!(drift_lines(f), vec![('-', 4), ('-', 5), ('+', 4), ('+', 5)]);
}

// A change removing a pre-existing (base) line in a later revision is its
// own real work — the base did not remove it — so it must show as a real
// deletion, never tagged drift and hidden.

#[test]
fn change_removing_a_base_line_in_a_later_revision_is_real() {
    let g = GitRepo::new();
    let base = ["use a;", "use b;", "fn keep() {}"];
    // The change adds feat.rs on top of the base, unchanged across revisions.
    let parent_m = snapshot(&g, &[("lib.rs", &body(&base))]);
    let m = snapshot(&g, &[("lib.rs", &body(&base)), ("feat.rs", &body(&["F"]))]);
    // The base drifts (keep() body changes); the change ALSO drops `use b;` at n.
    let drifted = ["use a;", "use b;", "fn keep() { work(); }"];
    let dropped = ["use a;", "fn keep() { work(); }"];
    let parent_n = snapshot(&g, &[("lib.rs", &body(&drifted))]);
    let n = snapshot(
        &g,
        &[("lib.rs", &body(&dropped)), ("feat.rs", &body(&["F"]))],
    );

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "lib.rs").expect("lib.rs stays in the list");
    // The keep() body change is base drift, tagged and excluded from real_dels.
    let real_dels: Vec<&str> = f
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.kind == LineKind::Del && !l.drift)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(real_dels, vec!["use b;"], "the change's deletion is real");
    assert_eq!(f.deletions, 1);
}

// Line-level limitation, asserted as a safe invariant: on runs of identical
// lines the two diffs can anchor a duplicate differently, so some base
// movement may show as a real edit (counts a little high). That is the safe
// direction — the change's own edit is still shown, never hidden as drift.

#[test]
fn duplicate_lines_never_hide_the_changes_edit() {
    let g = GitRepo::new();
    let parent_m = snapshot(&g, &[("f.rs", &body(&["b", "c"]))]);
    let m = snapshot(&g, &[("f.rs", &body(&["b", "c", "SAME"]))]);
    let parent_n = snapshot(&g, &[("f.rs", &body(&["c", "a", "c"]))]);
    let n = snapshot(&g, &[("f.rs", &body(&["c", "a", "c", "DIFF"]))]);

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "f.rs").expect("tagged f.rs");
    let real: Vec<&str> = f
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.kind != LineKind::Context && !l.drift)
        .map(|l| l.text.as_str())
        .collect();
    assert!(real.contains(&"SAME"), "the change's deletion is shown");
    assert!(real.contains(&"DIFF"), "the change's addition is shown");
}

// ---------------------------------------------------------------------------
// Property 8 — an isolated drift region is dropped; the file stays for its
// real edit, but the drift hunk does not drive a rendered region.

#[test]
fn isolated_drift_hunk_is_dropped_real_hunk_kept() {
    let g = GitRepo::new();
    let twelve = |a: &str, l: &str| {
        body(&[
            a, "l2", "l3", "l4", "l5", "l6", "l7", "l8", "l9", "l10", "l11", l,
        ])
    };
    let parent_m = snapshot(&g, &[("f.rs", &twelve("l1", "l12"))]);
    let m = snapshot(&g, &[("f.rs", &twelve("l1m", "l12"))]);
    let parent_n = snapshot(&g, &[("f.rs", &twelve("l1", "l12x"))]);
    let n = snapshot(&g, &[("f.rs", &twelve("l1n", "l12x"))]);

    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    // Plain: two hunks far apart — the real edit at top, the drift at bottom.
    assert_eq!(file(&plain, "f.rs").expect("plain f.rs").hunks.len(), 2);

    let f = file(&tagged, "f.rs").expect("file kept for its real edit");
    assert_eq!(f.hunks.len(), 1, "the all-drift hunk is dropped");
    assert_eq!((f.additions, f.deletions), (1, 1));
    assert!(
        drift_lines(f).is_empty(),
        "no drift renders in the kept hunk"
    );
    // Line 1 is the real edit; line 12 is the drift.
    assert_eq!(f.hunks[0].new_start, 1);
}

// ---------------------------------------------------------------------------
// Property 9 — coordinate correctness: the change's own edit shifts the drift
// to a different line number than it had in the parents, and it is still
// recognised (a naive parent-line comparison would tag the wrong line).

#[test]
fn drift_is_found_after_the_changes_edits_shift_it() {
    let g = GitRepo::new();
    // After X is inserted, E sits at line 6 in the interdiff but at line 5
    // in the parents.
    let parent_m = snapshot(&g, &[("f.rs", &body(&["A", "B", "C", "D", "E"]))]);
    let m = snapshot(&g, &[("f.rs", &body(&["X", "A", "B", "Cm", "D", "E"]))]);
    let parent_n = snapshot(&g, &[("f.rs", &body(&["A", "B", "C", "D", "Ex"]))]);
    let n = snapshot(&g, &[("f.rs", &body(&["X", "A", "B", "Cn", "D", "Ex"]))]);

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "f.rs").expect("file kept for its real edit");

    // E→Ex is drift at its *shifted* line 6; Cm→Cn is the real edit, line 4.
    assert_eq!(drift_lines(f), vec![('-', 6), ('+', 6)]);
    assert_eq!((f.additions, f.deletions), (1, 1));
}

// ---------------------------------------------------------------------------
// Property 10 — conservative on overlap: when the change's real edit touches
// the same lines the base moved, it is shown as a real edit, not drift.

#[test]
fn overlapping_edit_is_real_not_drift() {
    let g = GitRepo::new();
    let parent_m = snapshot(&g, &[("f.rs", &body(&["A", "B", "C", "D", "E"]))]);
    let m = snapshot(&g, &[("f.rs", &body(&["A", "B", "Cm", "D", "E"]))]);
    let parent_n = snapshot(&g, &[("f.rs", &body(&["A", "B", "Cx", "D", "E"]))]); // base also edits C
    let n = snapshot(&g, &[("f.rs", &body(&["A", "B", "Cn", "D", "E"]))]); // C2 edits the moved C

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "f.rs").expect("file kept (real edit)");
    assert!(drift_lines(f).is_empty(), "overlapping edit is not drift");
    assert_eq!((f.additions, f.deletions), (1, 1));
}

#[test]
fn file_added_or_deleted_by_the_rebase_is_dropped() {
    let g = GitRepo::new();
    let feat = body(&["C2 line"]);
    let cfg = body(&["added by the base"]);
    let old = body(&["removed by the base"]);

    let parent_m = snapshot(&g, &[("keep.rs", &body(&["k"]))]);
    let m = snapshot(&g, &[("keep.rs", &body(&["k"])), ("feat.rs", &feat)]);
    let parent_n = snapshot(&g, &[("keep.rs", &body(&["k"])), ("cfg.toml", &cfg)]);
    let n = snapshot(
        &g,
        &[
            ("keep.rs", &body(&["k"])),
            ("cfg.toml", &cfg),
            ("feat.rs", &feat),
        ],
    );
    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    assert!(file(&plain, "cfg.toml").is_some(), "plain leaks the add");
    assert!(tagged.files.is_empty(), "added-by-rebase file drops out");

    let parent_m = snapshot(&g, &[("keep.rs", &body(&["k"])), ("old.rs", &old)]);
    let m = snapshot(
        &g,
        &[
            ("keep.rs", &body(&["k"])),
            ("old.rs", &old),
            ("feat.rs", &feat),
        ],
    );
    let parent_n = snapshot(&g, &[("keep.rs", &body(&["k"]))]);
    let n = snapshot(&g, &[("keep.rs", &body(&["k"])), ("feat.rs", &feat)]);
    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    assert!(file(&plain, "old.rs").is_some(), "plain leaks the delete");
    assert!(tagged.files.is_empty(), "deleted-by-rebase file drops out");
}

// ---------------------------------------------------------------------------
// A rename the change made, of a file the base movement never touched, is
// real work: the rename and its edits render plain, never drift.

#[test]
fn changes_own_rename_is_left_as_a_plain_diff() {
    let g = GitRepo::new();
    let long = numbered(40);
    let tweaked = edit(&long, 1, "line 1 (touched)");

    let parent_m = snapshot(
        &g,
        &[("a.rs", &body(&long)), ("base.rs", &body(&["A", "B"]))],
    );
    let m = snapshot(
        &g,
        &[("a.rs", &body(&long)), ("base.rs", &body(&["A", "B"]))],
    );
    let parent_n = snapshot(
        &g,
        &[("a.rs", &body(&long)), ("base.rs", &body(&["A", "Bx"]))],
    );
    // n renames a.rs → b.rs (with a tweak) and carries the base drift.
    let n = snapshot(
        &g,
        &[("b.rs", &body(&tweaked)), ("base.rs", &body(&["A", "Bx"]))],
    );

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let renamed = file(&tagged, "b.rs").expect("renamed file present");
    assert_eq!(renamed.status, FileStatus::Renamed);
    assert!(
        drift_lines(renamed).is_empty(),
        "renamed file is not drift-processed"
    );
    assert!(file(&tagged, "base.rs").is_none(), "base.rs drift dropped");
}

// ---------------------------------------------------------------------------
// A rename the base movement itself made is drift like any other base edit:
// the interdiff entry drops when nothing real remains, and a base content
// tweak riding the rename is tagged.

#[test]
fn file_renamed_by_the_rebase_is_dropped() {
    let g = GitRepo::new();
    let long = numbered(40);
    let feat = body(&["C2 line"]);

    // Pure base rename a.rs → b.rs; the change's delta (feat.rs) is unchanged.
    let parent_m = snapshot(&g, &[("a.rs", &body(&long))]);
    let m = snapshot(&g, &[("a.rs", &body(&long)), ("feat.rs", &feat)]);
    let parent_n = snapshot(&g, &[("b.rs", &body(&long))]);
    let n = snapshot(&g, &[("b.rs", &body(&long)), ("feat.rs", &feat)]);
    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let leaked = file(&plain, "b.rs").expect("plain leaks the rename");
    assert_eq!(leaked.status, FileStatus::Renamed);
    assert!(tagged.files.is_empty(), "base rename drops out");

    // The same rename plus a base content tweak: still nothing real.
    let tweaked = edit(&long, 5, "line 5 (rebased)");
    let parent_n = snapshot(&g, &[("b.rs", &body(&tweaked))]);
    let n = snapshot(&g, &[("b.rs", &body(&tweaked)), ("feat.rs", &feat)]);
    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    assert!(file(&plain, "b.rs").is_some(), "plain leaks the tweak");
    assert!(tagged.files.is_empty(), "base rename + tweak drops out");
}

#[test]
fn base_rename_keeps_the_changes_real_edit_and_tags_drift() {
    let g = GitRepo::new();
    let long = numbered(40);
    // The change edits line 20 at both revisions (differently); the base
    // renames a.rs → b.rs and tweaks line 18 — near enough to share a hunk.
    let at_m = edit(&long, 20, "EDIT v1");
    let base_n = edit(&long, 18, "line 18 (rebased)");
    let at_n = edit(&base_n, 20, "EDIT v2");

    let parent_m = snapshot(&g, &[("a.rs", &body(&long))]);
    let m = snapshot(&g, &[("a.rs", &body(&at_m))]);
    let parent_n = snapshot(&g, &[("b.rs", &body(&base_n))]);
    let n = snapshot(&g, &[("b.rs", &body(&at_n))]);

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "b.rs").expect("file kept for its real edit");
    assert_eq!(f.status, FileStatus::Renamed);
    assert_eq!(f.old_path.as_deref(), Some("a.rs"));
    // The base tweak at line 18 is drift; the change's v1 → v2 edit is real.
    assert_eq!(drift_lines(f), vec![('-', 18), ('+', 18)]);
    assert_eq!((f.additions, f.deletions), (1, 1));
}

// ---------------------------------------------------------------------------
// Path re-keying: base movement is found under the file's per-side names,
// so it is contained even inside a file the change renamed.

#[test]
fn base_edit_is_found_inside_a_file_the_change_renamed() {
    let g = GitRepo::new();
    let long = numbered(40);
    let drifted = edit(&long, 5, "line 5 (rebased)");

    // The change renamed a.rs → b.rs at both revisions; the base modified
    // a.rs. The interdiff shows b.rs as modified — every edit the base's.
    let parent_m = snapshot(&g, &[("a.rs", &body(&long))]);
    let m = snapshot(&g, &[("b.rs", &body(&long))]);
    let parent_n = snapshot(&g, &[("a.rs", &body(&drifted))]);
    let n = snapshot(&g, &[("b.rs", &body(&drifted))]);

    let (plain, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    assert!(file(&plain, "b.rs").is_some(), "plain leaks the base edit");
    assert!(
        tagged.files.is_empty(),
        "base edit under the change's rename drops out"
    );
}

// The change's rename between revisions is real work and must stay visible
// even when every content edit inside it is base drift (the bug gerrit's
// implicitRename flag fixed).

#[test]
fn changes_rename_survives_an_all_drift_interdiff() {
    let g = GitRepo::new();
    let long = numbered(40);
    let drifted = edit(&long, 5, "line 5 (rebased)");

    // The base modified a.rs; the change renamed it to b.rs at n only.
    let parent_m = snapshot(&g, &[("a.rs", &body(&long))]);
    let m = snapshot(&g, &[("a.rs", &body(&long))]);
    let parent_n = snapshot(&g, &[("a.rs", &body(&drifted))]);
    let n = snapshot(&g, &[("b.rs", &body(&drifted))]);

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "b.rs").expect("the change's rename stays");
    assert_eq!(f.status, FileStatus::Renamed);
    assert_eq!(f.old_path.as_deref(), Some("a.rs"));
    assert!(f.hunks.is_empty(), "the base's edit is all drift");
    assert_eq!((f.additions, f.deletions), (0, 0));
}

// The base deleting a file the change then re-adds under a new name is NOT a
// base rename: the parent names don't connect, so nothing is drift-processed
// and the change's deletions inside the re-add stay real (projecting a
// fabricated whole-file delete would claim them as drift).

#[test]
fn base_delete_with_change_readd_is_left_plain() {
    let g = GitRepo::new();
    let long = numbered(40);
    let mut readded = long.clone();
    readded.remove(9); // the change drops line 10 in the re-added copy

    let parent_m = snapshot(&g, &[("a.rs", &body(&long))]);
    let m = snapshot(&g, &[("a.rs", &body(&long))]);
    let parent_n = snapshot(&g, &[]); // base deletes a.rs
    let n = snapshot(&g, &[("b.rs", &body(&readded))]);

    let (_, tagged) = interdiff(&g, m, parent_m, n, parent_n);
    let f = file(&tagged, "b.rs").expect("the re-add stays");
    assert_eq!(f.status, FileStatus::Renamed);
    assert!(drift_lines(f).is_empty(), "nothing is claimed as drift");
    assert_eq!((f.additions, f.deletions), (0, 1));
}

// ---------------------------------------------------------------------------
// A vs-parent diff is never drift-processed: byte-for-byte the plain diff,
// no drift lines, even when stacked on a moved base.

#[test]
fn vs_parent_diff_is_never_drift_processed() {
    let g = GitRepo::new();
    let base = body(&["fn shared() {}", "// stable", "fn tail() {}"]);
    let moved = body(&["fn shared() {}", "// CHANGED", "fn tail() {}"]);
    let feat = body(&["pub const F: u8 = 1;"]);

    // C1 (base) is amended; C2 is rebased onto the moved base, revision 1.
    let parent0 = g.commit_full(
        &[g.root],
        &msg("base", "Ibase"),
        &[("shared.rs", &base)],
        &[],
    );
    let r0 = g.commit_full(
        &[parent0],
        &msg("feat: add F", "Ifeat"),
        &[("shared.rs", &base), ("feat.rs", &feat)],
        &[],
    );
    g.branch("feat", r0);
    let parent1 = g.commit_full(
        &[g.root],
        &msg("base", "Ibase"),
        &[("shared.rs", &moved)],
        &[],
    );
    let r1 = g.commit_full(
        &[parent1],
        &msg("feat: add F", "Ifeat"),
        &[("shared.rs", &moved), ("feat.rs", &feat)],
        &[],
    );
    g.branch("feat2", r1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, p0) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "push r0: {p0}");
    let (st, p1) = push(&server, &g, "feat2", "main");
    assert_eq!(st, 200, "push r1: {p1}");
    let change_id = member_id(&server, &p1, "Ifeat");

    // r1's vs-parent diff is the plain `parent1 → r1` tree diff: feat.rs added,
    // nothing else, no drift — its parent is the source of truth, not r0's.
    let (st, diff) = http_get(&server.url(&format!("/api/changes/{change_id}/revisions/1/diff")));
    assert_eq!(st, 200, "vs-parent diff: {diff}");
    let any_drift = diff["files"]
        .as_array()
        .expect("files array")
        .iter()
        .flat_map(|f| f["hunks"].as_array().into_iter().flatten())
        .flat_map(|h| h["lines"].as_array().into_iter().flatten())
        .any(|l| l["drift"].as_bool() == Some(true));
    assert!(!any_drift, "a vs-parent diff carries no drift lines");
    let paths: Vec<&str> = diff["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|f| f["path"].as_str().expect("path"))
        .collect();
    assert_eq!(
        paths,
        vec![COMMIT_MSG_PATH, "feat.rs"],
        "vs-parent shows only the change's added file plus the message"
    );
}

// ---------------------------------------------------------------------------
// End-to-end through the HTTP handler: a stacked change rebased onto an
// amended earlier change, its r0→r1 interdiff served by
// `/api/changes/{id}/revisions/{n}/diff?against={m}`. A pure rebase collapses
// to just `/COMMIT_MSG`.

#[test]
fn http_interdiff_contains_a_pure_rebase() {
    let g = GitRepo::new();
    let base_v0 = body(&["fn shared() {}", "// stable", "fn tail() {}"]);
    let base_v1 = body(&["fn shared() {}", "// CHANGED", "fn tail() {}"]);
    let feat = body(&["pub const F: u8 = 1;"]);

    let c1_r0 = g.commit_full(
        &[g.root],
        &msg("infra: shared", "Ibase"),
        &[("shared.rs", &base_v0)],
        &[],
    );
    let c2_r0 = g.commit_full(
        &[c1_r0],
        &msg("feat: add F", "Ifeat"),
        &[("shared.rs", &base_v0), ("feat.rs", &feat)],
        &[],
    );
    g.branch("feat", c2_r0);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, p0) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "push the stack: {p0}");
    let change_id = member_id(&server, &p0, "Ifeat");

    // Amend C1 (its body moves) and rebase C2 onto it with the *same* delta and
    // message — a pure rebase of C2. Re-push: C1 gets revision 1 (a real edit),
    // C2 gets revision 1 (pure rebase, new parent only).
    let c1_r1 = g.commit_full(
        &[g.root],
        &msg("infra: shared", "Ibase"),
        &[("shared.rs", &base_v1)],
        &[],
    );
    let c2_r1 = g.commit_full(
        &[c1_r1],
        &msg("feat: add F", "Ifeat"),
        &[("shared.rs", &base_v1), ("feat.rs", &feat)],
        &[],
    );
    g.branch("feat", c2_r1);
    let (st, p1) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "re-push: {p1}");

    let (st, detail) = http_get(&server.url(&format!("/api/changes/{change_id}")));
    assert_eq!(st, 200, "change detail: {detail}");
    let revs = detail["revisions"].as_array().expect("revisions");
    assert_eq!(revs.len(), 2, "C2 has two revisions");
    assert_eq!(revs[1]["number"].as_u64(), Some(1), "revisions are 0-based");

    // The r0 → r1 interdiff: shared.rs is entirely drift (C1's amendment) and
    // drops out, so only the (unchanged) commit message remains.
    let (st, diff) = http_get(&server.url(&format!(
        "/api/changes/{change_id}/revisions/1/diff?against=0"
    )));
    assert_eq!(st, 200, "interdiff: {diff}");
    let paths: Vec<&str> = diff["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|f| f["path"].as_str().expect("path"))
        .collect();
    assert_eq!(
        paths,
        vec![COMMIT_MSG_PATH],
        "a pure rebase shows only the commit message"
    );
}
