//! Rebase-aware interdiffs: detect and contain "drift" — the parts of an
//! interdiff `m → n` caused by the change's base moving (a rebase) rather
//! than by the change's own edits (docs/api.md "Rebase-aware interdiffs").
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
//! drift.
//!
//! The projection ([`project_clipped`] / [`drift_ranges`]) is the bug-prone
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

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Result, anyhow};
use git2::{Patch, Repository, Tree};

use nit_types::diff::{Diff, Line};
use nit_types::enums::LineKind;

use super::diff;

/// A 0-based, half-open line range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: u64,
    end: u64,
}

impl Span {
    fn len(self) -> u64 {
        self.end - self.start
    }

    fn contains(self, point: u64) -> bool {
        self.start <= point && point < self.end
    }
}

/// A line-level edit: the A-range (old side) becomes the B-range (new side).
/// `JGit` semantics — a pure insertion has an empty A, a pure deletion an
/// empty B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edit {
    a: Span,
    b: Span,
}

/// The edit a single context-0 hunk header describes (a context-0 hunk is
/// exactly one contiguous change region). libgit2 reports a 1-based start and
/// a line count per side; an **empty** range (count 0) reports the position
/// *after which* the change sits, so its 0-based start is the reported start,
/// while a non-empty range's is `start - 1`.
fn edit_from_header(old_start: u64, old_lines: u64, new_start: u64, new_lines: u64) -> Edit {
    let span = |start: u64, lines: u64| {
        let begin = if lines == 0 { start } else { start - 1 };
        Span {
            start: begin,
            end: begin + lines,
        }
    };
    Edit {
        a: span(old_start, old_lines),
        b: span(new_start, new_lines),
    }
}

/// One edit per hunk of a context-0 patch of `old → new` (both already known
/// non-binary).
fn buffer_edits(old: &[u8], new: &[u8]) -> Result<Vec<Edit>> {
    let patch = Patch::from_buffers(old, None, new, None, Some(&mut diff::diff_opts(0)))?;
    (0..patch.num_hunks())
        .map(|h| {
            let (hunk, _) = patch.hunk(h)?;
            Ok(edit_from_header(
                u64::from(hunk.old_start()),
                u64::from(hunk.old_lines()),
                u64::from(hunk.new_start()),
                u64::from(hunk.new_lines()),
            ))
        })
        .collect()
}

fn net_delta(e: &Edit) -> i64 {
    i64::try_from(e.b.len()).unwrap_or(i64::MAX) - i64::try_from(e.a.len()).unwrap_or(i64::MAX)
}

/// Map the parts of `pos` that the change's own edits (`mappings`) did **not**
/// touch into the mappings' B-coordinate space, shifting each surviving
/// sub-range by the running insert/delete delta of the mappings before it.
/// A part of `pos` covered by a mapping's A-range is dropped — the change
/// edited those lines, so they show as a real edit, not drift (gerrit's
/// `OmitPositionOnConflict`, refined to line granularity: a base edit that
/// straddles one of the change's own lines still contributes its untouched
/// lines).
///
/// `mappings` must be ascending by `a.start` and disjoint — `buffer_edits`
/// (one edit per ascending hunk) yields them that way.
fn project_clipped(pos: Span, mappings: &[Edit]) -> Vec<Span> {
    debug_assert!(
        mappings.windows(2).all(|w| w[0].a.end <= w[1].a.start),
        "mappings must be ascending and disjoint"
    );
    let mut out = Vec::new();
    let mut cursor = pos.start; // start of the next not-yet-covered gap
    let mut shift: i64 = 0; // net delta of the mappings before `cursor`
    let mut emit = |from: u64, to: u64, shift: i64| {
        let shifted = |x: u64| u64::try_from(i64::try_from(x).ok()? + shift).ok();
        if from < to
            && let (Some(start), Some(end)) = (shifted(from), shifted(to))
        {
            out.push(Span { start, end });
        }
    };
    for m in mappings {
        if m.a.start >= pos.end {
            break;
        }
        if m.a.end <= cursor {
            shift += net_delta(m);
            continue;
        }
        emit(cursor, m.a.start, shift); // the untouched gap before this edit
        cursor = m.a.end; // step over the change's own edited region
        shift += net_delta(m);
    }
    emit(cursor, pos.end, shift);
    out
}

/// Project every base-movement (`pvp`) edit into the interdiff's `m`/`n` line
/// coordinates, independently per side: the A-range through the change's own
/// delta at `m` (`ovp`, `parent(m) → m`) and the B-range through its delta at
/// `n` (`nvp`, `parent(n) → n`). Per-side clipping keeps the lines the change
/// didn't touch, so an edit the diff folded across one of its own lines still
/// yields its drifted lines. Returns the drift line ranges on the old (`m`)
/// and new (`n`) sides of the interdiff.
fn drift_ranges(pvp: &[Edit], ovp: &[Edit], nvp: &[Edit]) -> (Vec<Span>, Vec<Span>) {
    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();
    for e in pvp {
        old_ranges.extend(project_clipped(e.a, ovp));
        new_ranges.extend(project_clipped(e.b, nvp));
    }
    (old_ranges, new_ranges)
}

/// True if the 1-based `line` falls inside any 0-based span.
fn in_ranges(ranges: &[Span], line: u64) -> bool {
    line >= 1 && ranges.iter().any(|r| r.contains(line - 1))
}

/// The file's blob bytes in `tree`: `Some(empty)` when the path is absent
/// (added/deleted across the four trees), `None` when it is binary (the
/// caller then leaves the file as a plain diff).
fn blob_bytes(repo: &Repository, tree: &Tree, path: &Path) -> Option<Vec<u8>> {
    let Ok(entry) = tree.get_path(path) else {
        return Some(Vec::new());
    };
    let blob = repo.find_blob(entry.id()).ok()?;
    if blob.is_binary() {
        return None;
    }
    Some(blob.content().to_vec())
}

/// Every path a tree diff touches, as `(name in old, name in new)` — the two
/// differ exactly when rename detection paired a delete with an add.
fn moves(repo: &Repository, old: &Tree, new: &Tree) -> Result<Vec<(String, String)>> {
    let diff = diff::git_diff(repo, old, new, None)?;
    let name = |f: git2::DiffFile<'_>| f.path().map(|p| p.to_string_lossy().into_owned());
    Ok(diff
        .deltas()
        .filter_map(|d| Some((name(d.old_file())?, name(d.new_file())?)))
        .collect())
}

/// One interdiff file's drifted line ranges, on the old (`m`) and new (`n`)
/// sides.
///
/// The blobs are the file's content in `parent(m)`, `m`, `parent(n)` and `n`,
/// each read under its name in that tree.
fn file_drift(bpm: &[u8], bm: &[u8], bpn: &[u8], bn: &[u8]) -> Result<(Vec<Span>, Vec<Span>)> {
    // parent(m) → m and parent(n) → n are the change's own delta at each
    // revision; parent(m) → parent(n) is the base movement. Projecting the
    // base movement through those deltas gives the drifted lines, in the
    // interdiff's own m/n coordinates.
    let ovp = buffer_edits(bpm, bm)?;
    let nvp = buffer_edits(bpn, bn)?;
    let pvp = buffer_edits(bpm, bpn)?;
    Ok(drift_ranges(&pvp, &ovp, &nvp))
}

/// Whether any line of `edit` escapes the drift ranges — an edit the change
/// made itself, which the reviewer must still see.
fn escapes_drift(edit: &Edit, old: &[Span], new: &[Span]) -> bool {
    let free = |span: Span, ranges: &[Span]| {
        (span.start..span.end).any(|l| !ranges.iter().any(|r| r.contains(l)))
    };
    free(edit.a, old) || free(edit.b, new)
}

fn is_real_change(line: &Line) -> bool {
    matches!(line.kind, LineKind::Add | LineKind::Del) && !line.drift
}

/// An interdiff's rebase drift, resolved from context-0 edit spans alone —
/// before any file is rendered (docs/api.md "Rebase-aware interdiffs"). A
/// file the base movement fully explains is named in [`Drift::skip`] and so
/// never costs a patch build, which is why the analysis runs first: a rebase
/// over a long base moves far more files than the change itself touches, and
/// rendering them only to discard them is the bulk of the work.
///
/// A file in neither map is left plain — it carries no drift, or it is
/// binary, or its blobs, per-file diff or parent names did not resolve.
#[derive(Default)]
pub struct Drift {
    /// Drifted line ranges, old (`m`) and new (`n`) side, per file that keeps
    /// a real edit.
    tagged: HashMap<String, (Vec<Span>, Vec<Span>)>,
    /// Files whose every edit is base movement.
    skip: HashSet<String>,
}

impl Drift {
    /// The files the render must not build a patch for.
    #[must_use]
    pub fn skip(&self) -> &HashSet<String> {
        &self.skip
    }

    /// Mark each analysed file's drift lines in place, drop its fully-drift
    /// hunks and recount its non-drift totals. Leaves every other file
    /// byte-identical, so a same-parent interdiff is untouched.
    pub fn tag(&self, diff: &mut Diff) {
        for file in &mut diff.files {
            let Some((old_ranges, new_ranges)) = self.tagged.get(&file.path) else {
                continue;
            };
            let mut any_drift = false;
            for line in file.hunks.iter_mut().flat_map(|h| h.lines.iter_mut()) {
                let drift = match line.kind {
                    LineKind::Del => line.old.is_some_and(|l| in_ranges(old_ranges, l)),
                    LineKind::Add => line.new.is_some_and(|l| in_ranges(new_ranges, l)),
                    LineKind::Context => false,
                };
                if drift {
                    line.drift = true;
                    any_drift = true;
                }
            }
            if !any_drift {
                continue;
            }
            // Region selection follows the change's own real edits.
            file.hunks.retain(|h| h.lines.iter().any(is_real_change));
            let (mut additions, mut deletions) = (0u64, 0u64);
            for line in file.hunks.iter().flat_map(|h| &h.lines) {
                match line.kind {
                    LineKind::Add if !line.drift => additions += 1,
                    LineKind::Del if !line.drift => deletions += 1,
                    _ => {}
                }
            }
            file.additions = additions;
            file.deletions = deletions;
        }
    }
}

/// Resolve the drift of `interdiff` (`tree(m) → tree(n)`, built by the caller
/// so its tree diff and rename detection are paid once). `only` bounds the
/// analysis to a single file, matching the render's own bound. The caller
/// invokes this only when `parent(m) != parent(n)`.
///
/// Best-effort and per-file: a file that is binary, whose blobs cannot be
/// read, whose per-file diff fails, or whose parent names the base movement
/// does not connect is left plain (the others are still contained). So a
/// failure never leaves a half-tagged file behind, and a returned error means
/// nothing is tagged at all (the caller serves the plain interdiff).
///
/// # Errors
/// When git cannot diff the two parents, or a delta vanishes mid-walk.
pub fn analyze(
    repo: &Repository,
    interdiff: &git2::Diff,
    m_sha: &str,
    parent_m_sha: &str,
    n_sha: &str,
    parent_n_sha: &str,
    only: Option<&str>,
) -> Result<Drift> {
    let mut drift = Drift::default();
    let (Some(tree_m), Some(tree_n), Some(parent_m), Some(parent_n)) = (
        diff::commit_tree(repo, m_sha),
        diff::commit_tree(repo, n_sha),
        diff::commit_tree(repo, parent_m_sha),
        diff::commit_tree(repo, parent_n_sha),
    ) else {
        return Ok(drift); // A tree won't resolve → leave the interdiff plain.
    };

    let base: HashMap<String, String> = moves(repo, &parent_m, &parent_n)?.into_iter().collect();
    if base.is_empty() {
        return Ok(drift);
    }
    // Each side's names read backwards, tree → parent: the seam that finds a
    // file under the name it was renamed away from.
    let from_parent = |pairs: Vec<(String, String)>| -> HashMap<String, String> {
        pairs.into_iter().map(|(old, new)| (new, old)).collect()
    };
    let in_parent_m = from_parent(moves(repo, &parent_m, &tree_m)?);
    let in_parent_n = from_parent(moves(repo, &parent_n, &tree_n)?);

    for idx in 0..interdiff.deltas().len() {
        let delta = interdiff
            .get_delta(idx)
            .ok_or_else(|| anyhow!("interdiff delta {idx} vanished"))?;
        let (path, old_path) = diff::delta_path(&delta);
        if only.is_some_and(|p| p != path) {
            continue;
        }
        let name_m = old_path.as_deref().unwrap_or(&path);
        let name_n = path.as_str();
        let name_pm = in_parent_m.get(name_m).map_or(name_m, String::as_str);
        let name_pn = in_parent_n.get(name_n).map_or(name_n, String::as_str);
        // The base movement must itself carry one parent name to the other.
        // Anything else (the base deleted the file, rename detection
        // disagreed) is left plain: diffing unrelated parent blobs could
        // claim the change's real edits as drift.
        if base.get(name_pm).map(String::as_str) != Some(name_pn) {
            continue;
        }
        // Gerrit's implicitRename: a rename either side's delta produced is
        // the change's own and stays visible even when fully drifted.
        let own_rename = old_path.is_some() && (name_pm != name_m || name_pn != name_n);
        let blob = |tree: &Tree, name: &str| blob_bytes(repo, tree, Path::new(name));
        let (Some(bpm), Some(bm), Some(bpn), Some(bn)) = (
            blob(&parent_m, name_pm),
            blob(&tree_m, name_m),
            blob(&parent_n, name_pn),
            blob(&tree_n, name_n),
        ) else {
            continue; // Binary on some side — leave plain.
        };
        // The file's own m → n edits decide whether anything survives the base
        // movement — the verdict the rendered lines would give, without
        // rendering them.
        let analysed = file_drift(&bpm, &bm, &bpn, &bn).and_then(|ranges| {
            let own = buffer_edits(&bm, &bn)?;
            let real = own.iter().any(|e| escapes_drift(e, &ranges.0, &ranges.1));
            Ok((ranges, real))
        });
        match analysed {
            Ok((ranges, real)) if real || own_rename => {
                drift.tagged.insert(path, ranges);
            }
            Ok(_) => {
                drift.skip.insert(path);
            }
            // Leave just this file plain; the rest are still contained.
            Err(e) => tracing::warn!("drift analysis skipped for {path}: {e:#}"),
        }
    }
    Ok(drift)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u64, end: u64) -> Span {
        Span { start, end }
    }

    fn edit(a: (u64, u64), b: (u64, u64)) -> Edit {
        Edit {
            a: span(a.0, a.1),
            b: span(b.0, b.1),
        }
    }

    #[test]
    fn edit_from_header_covers_every_range_shape() {
        // Replace 2 old lines (3,4) with 3 new (3,4,5).
        assert_eq!(edit_from_header(3, 2, 3, 3), edit((2, 4), (2, 5)));
        // Pure insertion of 2 lines after old line 5; pure deletion of 3,4.
        assert_eq!(edit_from_header(5, 0, 6, 2), edit((5, 5), (5, 7)));
        assert_eq!(edit_from_header(3, 2, 2, 0), edit((2, 4), (2, 2)));
        // Whole file added / deleted (the empty side has start 0).
        assert_eq!(edit_from_header(0, 0, 1, 3), edit((0, 0), (0, 3)));
        assert_eq!(edit_from_header(1, 3, 0, 0), edit((0, 3), (0, 0)));
    }

    #[test]
    fn project_clipped_shifts_an_uncovered_position() {
        // +2 at the top shifts a later position down by 2; a 3-line delete
        // before it shifts up by 3.
        assert_eq!(
            project_clipped(span(5, 6), &[edit((0, 0), (0, 2))]),
            vec![span(7, 8)]
        );
        assert_eq!(
            project_clipped(span(8, 9), &[edit((5, 8), (5, 5))]),
            vec![span(5, 6)]
        );
    }

    #[test]
    fn project_clipped_handles_after_and_full_cover() {
        assert_eq!(
            project_clipped(span(2, 3), &[edit((5, 8), (5, 8))]),
            vec![span(2, 3)]
        );
        // A position inside the change's own edit is dropped, not drift.
        assert!(project_clipped(span(6, 7), &[edit((5, 8), (5, 8))]).is_empty());
    }

    #[test]
    fn project_clipped_keeps_the_part_outside_the_changes_edit() {
        // The fix for drift the diff folds across one of the change's own
        // lines: the base region straddles that edit [5,8), and the untouched
        // remainder still projects (size-neutral mapping ⇒ no shift).
        let m = [edit((5, 8), (5, 8))];
        assert_eq!(project_clipped(span(4, 6), &m), vec![span(4, 5)]);
        assert_eq!(project_clipped(span(7, 9), &m), vec![span(8, 9)]);
        // An interior edit by the change splits the base region in two.
        assert_eq!(
            project_clipped(span(1, 9), &[edit((4, 5), (4, 5))]),
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

    #[test]
    fn in_ranges_is_one_based_against_zero_based_spans() {
        let ranges = [span(1, 3)]; // 0-based indices 1,2 → 1-based lines 2,3
        assert!(!in_ranges(&ranges, 1));
        assert!(in_ranges(&ranges, 2));
        assert!(in_ranges(&ranges, 3));
        assert!(!in_ranges(&ranges, 4));
        assert!(!in_ranges(&ranges, 0));
    }
}
