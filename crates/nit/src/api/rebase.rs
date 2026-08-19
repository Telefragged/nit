//! Rebase-aware interdiffs: detect and contain "drift".
//!
//! Drift is the parts of an interdiff `m → n` caused by the change's base
//! moving (a rebase) rather than by the change's own edits.
//!
//! Gerrit's mechanism, line-level: diff the two parents
//! (`parent(m) → parent(n)`) to find the base movement, then project those
//! edits into the interdiff's `m`/`n` line coordinates through the change's
//! own delta at each revision (`parent(m) → tree(m)` and
//! `parent(n) → tree(n)`), so a base edit is recognised wherever the
//! change's own edits shifted it (spec property 9). Projection clips out the
//! lines the change also touched, so its own edit is shown as a real edit,
//! never claimed as drift (property 10), and an interdiff line the base
//! movement does not explain — including the change removing a pre-existing
//! line in a later revision — stays real.
//!
//! File identity across renames follows gerrit's path re-keying: every blob
//! is read under the file's name in its own tree, resolved through each
//! side's rename detection, so base movement is contained even inside a
//! file the change renamed, and a rename made wholly by the base is itself
//! drift. A rename the tree diff does not pair — past [`diff::git_diff`]'s
//! ceiling only identical blobs pair — leaves that file's base movement
//! unrecognised and shown as the change's work: extra churn, the safe
//! direction.
//!
//! Which files the interdiff shows at all is decided the same way gerrit's
//! `ModifiedFilesLoader` decides it: a file the change touched at neither
//! revision moved with the base alone, and is dropped before anything reads
//! it. Under a long rebase that is nearly the whole interdiff, so the line
//! analysis below runs over the change's own handful of files.
//!
//! The projection (`project_clipped` / `drift_ranges`) is the bug-prone
//! core gerrit shipped a false-negative in (2.15.0), and is unit-tested
//! below. It is **line-level**, with two inherent limitations matching
//! gerrit (the spec deems intraline/move detection out of scope):
//!
//! - On runs of identical lines (blank lines, `}`, repeated imports) the two
//!   diffs can anchor a duplicate differently, leaving a base-movement line
//!   shown as a real edit rather than drift — extra base churn, the safe
//!   direction.
//! - When the base *reorders* a line that the change also deletes, the
//!   line-level diff cannot tell "base moved line X" from "base deleted line
//!   X", so the change's deletion can be tagged drift. A deletion the base
//!   did **not** touch (the common "also drop this line" case) is unaffected
//!   and stays a real edit.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::path::Path;

use anyhow::Result;
use git2::{Delta, Oid, Repository, Tree};
use imara_diff::InternedInput;

use nit_types::diff::{Diff, DiffFile, Line};
use nit_types::domain::LineKind;
use nit_types::domain::Sha;

use super::diff;

/// A 0-based, half-open line range; `Edit` turns one into another.
///
/// [`imara_diff::Hunk`]'s `before`/`after`, which is already what the
/// projection speaks. A pure insertion has an empty `before`, a pure
/// deletion an empty `after`.
type Span = Range<u32>;
type Edit = imara_diff::Hunk;

/// One file's drifted lines, on the old (`m`) and new (`n`) side.
type DriftRanges = (Vec<Span>, Vec<Span>);

/// One edit per contiguous change region of `old → new`.
fn buffer_edits(old: &[u8], new: &[u8]) -> Vec<Edit> {
    diff::line_edits(&InternedInput::new(old, new))
}

fn net_delta(e: &Edit) -> i64 {
    i64::from(e.after.end - e.after.start) - i64::from(e.before.end - e.before.start)
}

/// Maps the parts of `pos` the mappings missed into B coordinates.
///
/// The parts of `pos` that the change's own edits (`mappings`) did **not**
/// touch move into the mappings' B-coordinate space, each surviving
/// sub-range shifted by the running insert/delete delta of the mappings
/// before it. A part of `pos` covered by a mapping's A-range is dropped —
/// the change edited those lines, so they show as a real edit, not drift
/// (gerrit's `OmitPositionOnConflict`, refined to line granularity: a base
/// edit that straddles one of the change's own lines still contributes its
/// untouched lines).
///
/// `mappings` must be ascending by `before.start` and disjoint —
/// `buffer_edits` (one edit per ascending hunk) yields them that way.
fn project_clipped(pos: &Span, mappings: &[Edit]) -> Vec<Span> {
    debug_assert!(
        mappings
            .windows(2)
            .all(|w| w[0].before.end <= w[1].before.start),
        "mappings must be ascending and disjoint"
    );
    let mut out = Vec::new();
    let mut cursor = pos.start; // start of the next not-yet-covered gap
    let mut shift: i64 = 0; // net delta of the mappings before `cursor`
    let mut emit = |from: u32, to: u32, shift: i64| {
        let shifted = |x: u32| u32::try_from(i64::from(x) + shift).ok();
        if from < to
            && let (Some(start), Some(end)) = (shifted(from), shifted(to))
        {
            out.push(start..end);
        }
    };
    for m in mappings {
        if m.before.start >= pos.end {
            break;
        }
        if m.before.end <= cursor {
            shift += net_delta(m);
            continue;
        }
        emit(cursor, m.before.start, shift);
        cursor = m.before.end;
        shift += net_delta(m);
    }
    emit(cursor, pos.end, shift);
    out
}

/// Projects every base-movement edit into the interdiff's coordinates.
///
/// Every `pvp` edit lands in the interdiff's `m`/`n` line coordinates,
/// independently per side: the A-range through the change's own delta at
/// `m` (`ovp`, `parent(m) → m`) and the B-range through its delta at `n`
/// (`nvp`, `parent(n) → n`). Per-side clipping keeps the lines the change
/// didn't touch, so an edit the diff folded across one of its own lines
/// still yields its drifted lines. Returns the drift line ranges on the old
/// (`m`) and new (`n`) sides of the interdiff.
fn drift_ranges(pvp: &[Edit], ovp: &[Edit], nvp: &[Edit]) -> DriftRanges {
    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();
    for e in pvp {
        old_ranges.extend(project_clipped(&e.before, ovp));
        new_ranges.extend(project_clipped(&e.after, nvp));
    }
    (old_ranges, new_ranges)
}

/// True if the 0-based `line` falls inside any span.
fn drifted(ranges: &[Span], line: u32) -> bool {
    ranges.iter().any(|r| r.contains(&line))
}

/// The oid of `path` in `tree`, zero when it is absent.
///
/// The null oid every git tree diff already uses for the missing side of an
/// add or delete, and what [`diff::blob_bytes`] reads as the empty text.
fn entry_oid(tree: &Tree, path: &str) -> Oid {
    tree.get_path(Path::new(path))
        .map_or(Oid::zero(), |e| e.id())
}

/// Every path a tree diff touches, as `(name in old, name in new)`.
///
/// The two differ exactly when rename detection paired a delete with an
/// add. `paths` bounds the diff as [`diff::git_diff`] takes it.
fn moves(
    repo: &Repository,
    old: &Tree,
    new: &Tree,
    paths: Option<&[String]>,
) -> Result<Vec<(String, String)>> {
    let diff = diff::git_diff(repo, old, new, paths)?;
    let name = |f: git2::DiffFile<'_>| f.path().map(|p| p.to_string_lossy().into_owned());
    Ok(diff
        .deltas()
        .filter_map(|d| Some((name(d.old_file())?, name(d.new_file())?)))
        .collect())
}

/// One interdiff file's drifted line ranges, old (`m`) and new (`n`).
///
/// The blobs are the file's content in `parent(m)`, `m`, `parent(n)` and `n`,
/// each read under its name in that tree.
fn file_drift(bpm: &[u8], bm: &[u8], bpn: &[u8], bn: &[u8]) -> DriftRanges {
    // parent(m) → m and parent(n) → n are the change's own delta at each
    // revision; parent(m) → parent(n) is the base movement. Projecting the
    // base movement through those deltas gives the drifted lines, in the
    // interdiff's own m/n coordinates.
    let ovp = buffer_edits(bpm, bm);
    let nvp = buffer_edits(bpn, bn);
    let pvp = buffer_edits(bpm, bpn);
    drift_ranges(&pvp, &ovp, &nvp)
}

fn is_real_change(line: &Line) -> bool {
    matches!(line.kind, LineKind::Add | LineKind::Del) && !line.drift
}

/// Marks the file's drifted lines, drops its fully-drift hunks and recounts
/// what is left.
///
/// Region selection follows the change's own real edits: a hunk the base
/// alone explains is not a region of this file the reviewer is looking at.
fn tag(file: &mut DiffFile, (old_ranges, new_ranges): &DriftRanges) {
    // The wire numbers lines from 1; the spans index from 0.
    let hit = |ranges: &[Span], n: Option<u64>| {
        n.and_then(|n| u32::try_from(n).ok()?.checked_sub(1))
            .is_some_and(|l| drifted(ranges, l))
    };
    for line in file.hunks.iter_mut().flat_map(|h| h.lines.iter_mut()) {
        line.drift = match line.kind {
            LineKind::Del => hit(old_ranges, line.old),
            LineKind::Add => hit(new_ranges, line.new),
            LineKind::Context => false,
        };
    }
    file.hunks.retain(|h| h.lines.iter().any(is_real_change));
    (file.additions, file.deletions) = diff::stats(&file.hunks);
}

/// The wire diff for `interdiff` (`tree(m) → tree(n)`) with its drift
/// contained.
///
/// The interdiff is built by the caller so its tree diff and rename
/// detection are paid once. `only` bounds the walk to a single file,
/// matching the render's own bound. The caller invokes this only when
/// `parent(m) != parent(n)`.
///
/// One pass, one verdict per file, each decided on the cheapest evidence
/// that settles it: a file the base moved on its own is dropped by name
/// alone, one the change left untouched at both revisions by its tree oids,
/// and only what survives is read and diffed — once, for its drift ranges
/// and its wire lines together. Best-effort — a file the change touched that
/// is binary, or whose parent names the base movement does not connect, is
/// rendered plain (the others are still contained). A returned error means
/// nothing is contained at all (the caller serves the plain interdiff).
///
/// # Errors
///
/// When a tree won't resolve, git cannot diff two of them, or a blob cannot
/// be read.
pub fn contain(
    repo: &Repository,
    interdiff: &git2::Diff,
    m: &Rev,
    n: &Rev,
    context: u32,
    only: Option<&str>,
) -> Result<Diff> {
    let (Some(tree_m), Some(tree_n), Some(parent_m), Some(parent_n)) = (
        diff::commit_tree(repo, m.commit),
        diff::commit_tree(repo, n.commit),
        diff::commit_tree(repo, m.parent),
        diff::commit_tree(repo, n.parent),
    ) else {
        anyhow::bail!("a revision's tree is missing");
    };

    // Each side's names read backwards, tree → parent: the seam that finds a
    // file under the name it was renamed away from.
    let from_parent = |pairs: Vec<(String, String)>| -> HashMap<String, String> {
        pairs.into_iter().map(|(old, new)| (new, old)).collect()
    };
    let in_parent_m = from_parent(moves(repo, &parent_m, &tree_m, None)?);
    let in_parent_n = from_parent(moves(repo, &parent_n, &tree_n, None)?);
    // Every name the change itself touched, on both sides of a rename it
    // made: the only files that can carry work of its own.
    let touched: BTreeSet<&str> = in_parent_m
        .iter()
        .chain(&in_parent_n)
        .flat_map(|(tree, parent)| [tree.as_str(), parent.as_str()])
        .collect();
    if touched.is_empty() {
        // The change holds no file at either revision, so the interdiff is
        // the base's throughout and the base movement need not be read to
        // say so — which an empty bound could not ask for anyway.
        return Ok(Diff { files: Vec::new() });
    }
    // A superset of the names the walk below looks up: what the change
    // touched, plus the far side of a rename the interdiff paired with one of
    // them. What the bound leaves out is a file the base moved on its own,
    // which no lookup asks about.
    let mut bound: BTreeSet<String> = touched.iter().map(|&name| name.to_owned()).collect();
    for delta in interdiff.deltas().filter(|d| d.status() == Delta::Renamed) {
        if let Some(file) = diff::delta_file(&delta)
            && let Some(old_path) = file.old_path
            && (touched.contains(file.path.as_str()) || touched.contains(old_path.as_str()))
        {
            bound.extend([file.path, old_path]);
        }
    }
    let bound: Vec<String> = bound.into_iter().collect();
    let base: HashMap<String, String> = moves(repo, &parent_m, &parent_n, Some(&bound))?
        .into_iter()
        .collect();

    let mut files = Vec::new();
    for delta in interdiff.deltas() {
        let Some(mut file) = diff::delta_file(&delta) else {
            continue;
        };
        if only.is_some_and(|p| p != file.path) {
            continue;
        }
        let name_m = file.old_path.as_deref().unwrap_or(&file.path);
        let name_n = file.path.as_str();
        // A file the change touched under neither name moved with the base
        // alone, so none of it is the change's to show (gerrit's
        // `isTouched`).
        if !touched.contains(name_m) && !touched.contains(name_n) {
            continue;
        }
        let name_pm = in_parent_m.get(name_m).map_or(name_m, String::as_str);
        let name_pn = in_parent_n.get(name_n).map_or(name_n, String::as_str);
        // The base movement must itself carry one parent name to the other.
        // Anything else (the base deleted the file, rename detection
        // disagreed) is left plain: diffing unrelated parent blobs could
        // claim the change's real edits as drift.
        if base.get(name_pm).map(String::as_str) != Some(name_pn) {
            files.push(diff::render_delta(repo, &delta, file, context)?);
            continue;
        }
        // Gerrit's implicitRename: a rename either side's delta produced is
        // the change's own and stays visible even when fully drifted.
        let own_rename = file.old_path.is_some() && (name_pm != name_m || name_pn != name_n);
        // The interdiff's own delta already carries the two tree(m)/tree(n) ids.
        let (oid_pm, oid_m) = (entry_oid(&parent_m, name_pm), delta.old_file().id());
        let (oid_pn, oid_n) = (entry_oid(&parent_n, name_pn), delta.new_file().id());
        // The change left this file exactly as it found it at both revisions,
        // so every line of the interdiff is the base's. Diffing would only
        // rediscover that: with no delta of the change's own to project
        // through, the drift ranges come out equal to the base movement and
        // nothing escapes them. Deciding it on tree oids alone keeps the
        // blobs unread.
        if oid_pm == oid_m && oid_pn == oid_n && !own_rename {
            continue;
        }
        let blob = |name: &str, oid| diff::blob_bytes(repo, name, oid);
        let (Some(bpm), Some(bm), Some(bpn), Some(bn)) = (
            blob(name_pm, oid_pm)?,
            blob(name_m, oid_m)?,
            blob(name_pn, oid_pn)?,
            blob(name_n, oid_n)?,
        ) else {
            // Binary on some side.
            files.push(diff::render_delta(repo, &delta, file, context)?);
            continue;
        };
        let ranges = file_drift(&bpm, &bm, &bpn, &bn);
        diff::fill_lines(&mut file, &bm, &bn, context);
        tag(&mut file, &ranges);
        // What the tagging left standing is the verdict: a file with no real
        // edit of the change's own is the base's work throughout. A rename
        // the change made is its own work even when every line inside it
        // drifted.
        if own_rename || !file.hunks.is_empty() {
            files.push(file);
        }
    }
    Ok(Diff { files })
}

/// A revision and the parent its diff is taken against.
///
/// The pair [`contain`] needs at each end of an interdiff, named so the two
/// cannot be swapped.
pub struct Rev<'a> {
    pub commit: &'a Sha,
    pub parent: &'a Sha,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32) -> Span {
        start..end
    }

    fn edit(before: (u32, u32), after: (u32, u32)) -> Edit {
        Edit {
            before: span(before.0, before.1),
            after: span(after.0, after.1),
        }
    }

    #[test]
    fn buffer_edits_span_every_range_shape() {
        let text = |lines: &[&str]| lines.join("\n").into_bytes();
        let base = text(&["a", "b", "c", "d", "e", ""]);
        // Every shape a span can take, since the projection reads both ends of
        // both sides: replace, insert (empty before), delete (empty after),
        // and each side empty for a whole-file add or delete.
        assert_eq!(
            buffer_edits(&base, &text(&["a", "b", "C", "D", "E", "e", ""])),
            vec![edit((2, 4), (2, 5))]
        );
        assert_eq!(
            buffer_edits(&base, &text(&["a", "b", "c", "d", "e", "f", "g", ""])),
            vec![edit((5, 5), (5, 7))]
        );
        assert_eq!(
            buffer_edits(&base, &text(&["a", "b", "e", ""])),
            vec![edit((2, 4), (2, 2))]
        );
        assert_eq!(
            buffer_edits(b"", &text(&["a", "b", "c", ""])),
            vec![edit((0, 0), (0, 3))]
        );
        assert_eq!(
            buffer_edits(&text(&["a", "b", "c", ""]), b""),
            vec![edit((0, 3), (0, 0))]
        );
    }

    #[test]
    fn project_clipped_shifts_an_uncovered_position() {
        // +2 at the top shifts a later position down by 2; a 3-line delete
        // before it shifts up by 3.
        assert_eq!(
            project_clipped(&span(5, 6), &[edit((0, 0), (0, 2))]),
            vec![span(7, 8)]
        );
        assert_eq!(
            project_clipped(&span(8, 9), &[edit((5, 8), (5, 5))]),
            vec![span(5, 6)]
        );
    }

    #[test]
    fn project_clipped_handles_after_and_full_cover() {
        assert_eq!(
            project_clipped(&span(2, 3), &[edit((5, 8), (5, 8))]),
            vec![span(2, 3)]
        );
        // A position inside the change's own edit is dropped, not drift.
        assert!(project_clipped(&span(6, 7), &[edit((5, 8), (5, 8))]).is_empty());
    }

    #[test]
    fn project_clipped_keeps_the_part_outside_the_changes_edit() {
        // The fix for drift the diff folds across one of the change's own
        // lines: the base region straddles that edit [5,8), and the untouched
        // remainder still projects (size-neutral mapping ⇒ no shift).
        let m = [edit((5, 8), (5, 8))];
        assert_eq!(project_clipped(&span(4, 6), &m), vec![span(4, 5)]);
        assert_eq!(project_clipped(&span(7, 9), &m), vec![span(8, 9)]);
        // An interior edit by the change splits the base region in two.
        assert_eq!(
            project_clipped(&span(1, 9), &[edit((4, 5), (4, 5))]),
            vec![span(1, 4), span(5, 9)]
        );
    }

    #[test]
    fn drift_ranges_shifts_with_the_changes_edits_and_clips_overlap() {
        // Property 9: base inserts a line; the change inserts 2 above it at both
        // revisions, so the drift lands 2 lines lower in m/n.
        let (old, new) = drift_ranges(
            &[edit((3, 3), (3, 4))],
            &[edit((0, 0), (0, 2))],
            &[edit((0, 0), (0, 2))],
        );
        assert!(old.is_empty()); // a pure insertion has no old-side line
        assert_eq!(new, vec![span(5, 6)]); // new index 5 == line 6

        // Property 10: the change edits the same line the base moved → not drift.
        let (old, new) = drift_ranges(
            &[edit((3, 4), (3, 4))],
            &[edit((3, 4), (3, 4))],
            &[edit((3, 4), (3, 4))],
        );
        assert!(old.is_empty() && new.is_empty());
    }
}
