//! Rebase-aware interdiffs: detect and contain "drift" — the parts of an
//! interdiff `m → n` caused by the change's base moving (a rebase) rather
//! than by the agent editing the change (docs/api.md "Rebase-aware
//! interdiffs").
//!
//! Gerrit's mechanism, line-level: diff the two parents
//! (`parent(m) → parent(n)`) to find the base movement, then project those
//! edits into the interdiff's `m`/`n` line coordinates through the change's
//! own delta at each revision (`parent(m) → tree(m)` and
//! `parent(n) → tree(n)`), so a base edit is recognised wherever the agent's
//! own edits shifted it (spec property 9). Projection clips out the lines the
//! agent also touched, so an agent edit is shown as a real change, never
//! claimed as drift (property 10), and an interdiff change the base movement
//! does not explain — including the agent removing a pre-existing line in a
//! later revision — stays a real change.
//!
//! The projection ([`project_clipped`] / [`drift_ranges`]) is the bug-prone
//! core gerrit shipped a false-negative in (2.15.0), and is unit-tested
//! below. It is **line-level**, with two inherent limitations matching
//! gerrit (the spec deems intraline/move detection out of scope):
//!
//! - On runs of identical lines (blank lines, `}`, repeated imports) the two
//!   diffs can anchor a duplicate differently, leaving a base-movement line
//!   shown as a real change rather than drift — extra base churn, the safe
//!   direction.
//! - When the base *reorders* a line that the agent also deletes, the
//!   line-level diff cannot tell "base moved line X" from "base deleted line
//!   X", so the agent's deletion can be tagged drift. A deletion the base
//!   did **not** touch (the common "also drop this line" case) is unaffected
//!   and stays a real change.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use git2::{DiffOptions, Patch, Repository, Tree};

use super::diff::{self, COMMIT_MSG_PATH};
use super::types;

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
    let mut opts = DiffOptions::new();
    opts.context_lines(0);
    let patch = Patch::from_buffers(old, None, new, None, Some(&mut opts))?;
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

/// The net line delta an edit introduces (added minus deleted lines).
fn net_delta(e: &Edit) -> i64 {
    i64::try_from(e.b.len()).unwrap_or(i64::MAX) - i64::try_from(e.a.len()).unwrap_or(i64::MAX)
}

/// Map the parts of `pos` that the change's own edits (`mappings`) did **not**
/// touch into the mappings' B-coordinate space, shifting each surviving
/// sub-range by the running insert/delete delta of the mappings before it.
/// A part of `pos` covered by a mapping's A-range is dropped — the agent
/// edited those lines, so they show as a real edit, not drift (gerrit's
/// `OmitPositionOnConflict`, refined to line granularity: a base edit that
/// straddles one agent-edited line still contributes its untouched lines).
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
            break; // this mapping and every later one is past `pos`
        }
        if m.a.end <= cursor {
            shift += net_delta(m); // entirely before the cursor
            continue;
        }
        emit(cursor, m.a.start, shift); // the untouched gap before this edit
        cursor = m.a.end; // step over the agent-edited region
        shift += net_delta(m);
    }
    emit(cursor, pos.end, shift);
    out
}

/// Project every base-movement (`pvp`) edit into the interdiff's `m`/`n` line
/// coordinates, independently per side: the A-range through the change's own
/// delta at `m` (`ovp`, `parent(m) → m`) and the B-range through its delta at
/// `n` (`nvp`, `parent(n) → n`). Per-side clipping keeps the lines the agent
/// didn't touch, so an edit the diff folded across an agent-edited line still
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

/// Paths touched by `old → new` (both sides, so renames/deletes are
/// covered) — the pre-filter for which interdiff files can carry drift.
fn changed_paths(repo: &Repository, old: &Tree, new: &Tree) -> Result<HashSet<String>> {
    let diff = repo.diff_tree_to_tree(Some(old), Some(new), None)?;
    let mut paths = HashSet::new();
    for delta in diff.deltas() {
        for file in [delta.old_file(), delta.new_file()] {
            if let Some(p) = file.path() {
                paths.insert(p.to_string_lossy().into_owned());
            }
        }
    }
    Ok(paths)
}

/// Tag one interdiff file with drift in place; returns `true` when the file
/// became fully drift (the caller drops it from the file list). Leaves the
/// file untouched (byte-identical) when it carries no drift.
fn tag_file(
    repo: &Repository,
    file: &mut types::DiffFile,
    parent_m: &Tree,
    tree_m: &Tree,
    parent_n: &Tree,
    tree_n: &Tree,
) -> Result<bool> {
    let path = Path::new(&file.path);
    let (Some(bpm), Some(bm), Some(bpn), Some(bn)) = (
        blob_bytes(repo, parent_m, path),
        blob_bytes(repo, tree_m, path),
        blob_bytes(repo, parent_n, path),
        blob_bytes(repo, tree_n, path),
    ) else {
        return Ok(false); // Binary on some side — leave plain.
    };
    // parent(m) → m and parent(n) → n are the change's own delta at each
    // revision; parent(m) → parent(n) is the base movement. Projecting the
    // base movement through the agent's deltas gives the drifted lines, in
    // the interdiff's own m/n coordinates.
    let ovp = buffer_edits(&bpm, &bm)?;
    let nvp = buffer_edits(&bpn, &bn)?;
    let pvp = buffer_edits(&bpm, &bpn)?;
    let (old_ranges, new_ranges) = drift_ranges(&pvp, &ovp, &nvp);

    let mut any_drift = false;
    for hunk in &mut file.hunks {
        for line in &mut hunk.lines {
            let drift = match line.kind.as_str() {
                "del" => line.old.is_some_and(|l| in_ranges(&old_ranges, l)),
                "add" => line.new.is_some_and(|l| in_ranges(&new_ranges, l)),
                _ => false,
            };
            if drift {
                line.drift = true;
                any_drift = true;
            }
        }
    }
    if !any_drift {
        return Ok(false); // No drift → byte-identical, leave untouched.
    }

    // Region selection follows the agent's real delta: keep a hunk only if
    // it carries a real changed line; an isolated all-drift hunk is dropped.
    file.hunks.retain(|h| h.lines.iter().any(is_real_change));
    // Recount over the survivors, excluding drift (one pass for both totals).
    let (mut additions, mut deletions) = (0u64, 0u64);
    for line in file.hunks.iter().flat_map(|h| &h.lines) {
        match line.kind.as_str() {
            "add" if !line.drift => additions += 1,
            "del" if !line.drift => deletions += 1,
            _ => {}
        }
    }
    file.additions = additions;
    file.deletions = deletions;
    Ok(file.hunks.is_empty())
}

fn is_real_change(line: &types::Line) -> bool {
    (line.kind == "add" || line.kind == "del") && !line.drift
}

/// Tag the interdiff `diff` (already rendered `tree(m) → tree(n)`) with
/// rebase drift in place: mark drift lines, drop fully-drift hunks, recount
/// the non-drift totals, and drop fully-drift files (docs/api.md
/// "Rebase-aware interdiffs"). A no-op for files the base movement does not
/// touch, so a same-parent interdiff is unchanged. The caller invokes this
/// only when `parent(m) != parent(n)`.
///
/// Best-effort and per-file: a file that is binary or renamed, whose blobs
/// cannot be read, or whose per-file diff fails is left as a plain diff (the
/// others are still contained); `/COMMIT_MSG` is never drift-processed. So a
/// failure never leaves a half-tagged file behind, and a returned error means
/// nothing was tagged at all (the caller serves the plain interdiff).
///
/// # Errors
/// When git cannot diff the two parents (before any file is touched).
pub fn tag_drift(
    repo: &Repository,
    diff: &mut types::Diff,
    m_sha: &str,
    parent_m_sha: &str,
    n_sha: &str,
    parent_n_sha: &str,
) -> Result<()> {
    let (Some(tree_m), Some(tree_n), Some(parent_m), Some(parent_n)) = (
        diff::commit_tree(repo, m_sha),
        diff::commit_tree(repo, n_sha),
        diff::commit_tree(repo, parent_m_sha),
        diff::commit_tree(repo, parent_n_sha),
    ) else {
        return Ok(()); // A tree won't resolve → leave the interdiff plain.
    };

    let drifted = changed_paths(repo, &parent_m, &parent_n)?;
    if drifted.is_empty() {
        return Ok(()); // Base unchanged between the parents — no drift.
    }

    let mut drop_files = Vec::new();
    for (idx, file) in diff.files.iter_mut().enumerate() {
        if file.path == COMMIT_MSG_PATH
            || file.binary
            || file.status == "renamed"
            || !drifted.contains(&file.path)
        {
            continue;
        }
        match tag_file(repo, file, &parent_m, &tree_m, &parent_n, &tree_n) {
            Ok(true) => drop_files.push(idx),
            Ok(false) => {}
            // Leave just this file plain; the rest are still contained.
            Err(e) => tracing::warn!("drift tagging skipped for {}: {e:#}", file.path),
        }
    }
    for idx in drop_files.into_iter().rev() {
        diff.files.remove(idx);
    }
    Ok(())
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
        // A mapping entirely after the position leaves it untouched.
        assert_eq!(
            project_clipped(span(2, 3), &[edit((5, 8), (5, 8))]),
            vec![span(2, 3)]
        );
        // A position wholly inside an agent edit is dropped (real, not drift).
        assert!(project_clipped(span(6, 7), &[edit((5, 8), (5, 8))]).is_empty());
    }

    #[test]
    fn project_clipped_keeps_the_part_outside_an_agent_edit() {
        // The fix for drift the diff folds across an agent-edited line: the
        // base region straddles the agent's edit [5,8), and the untouched
        // remainder still projects (size-neutral mapping ⇒ no shift).
        let m = [edit((5, 8), (5, 8))];
        assert_eq!(project_clipped(span(4, 6), &m), vec![span(4, 5)]); // prefix
        assert_eq!(project_clipped(span(7, 9), &m), vec![span(8, 9)]); // suffix
        // An interior agent edit splits the base region in two.
        assert_eq!(
            project_clipped(span(1, 9), &[edit((4, 5), (4, 5))]),
            vec![span(1, 4), span(5, 9)]
        );
    }

    #[test]
    fn drift_ranges_shifts_with_the_agents_edits_and_clips_overlap() {
        // Property 9: base inserts a line; the agent inserts 2 above it at both
        // revisions, so the drift lands 2 lines lower in m/n.
        let (old, new) = drift_ranges(
            &[edit((3, 3), (3, 4))],
            &[edit((0, 0), (0, 2))],
            &[edit((0, 0), (0, 2))],
        );
        assert!(old.is_empty()); // a pure insertion has no old-side line
        assert_eq!(new, vec![span(5, 6)]); // new index 5 == line 6

        // Property 10: the agent edits the same line the base moved → not drift.
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
