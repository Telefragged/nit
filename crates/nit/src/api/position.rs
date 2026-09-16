//! Line positions in one tree, carried into another through the line edits
//! between them.
//!
//! A position is a span of lines in a file of one tree. The edits
//! `old → new` of that file say where each of its lines went, so a span in
//! the old tree maps to a span in the new one: shifted by the lines the
//! edits before it inserted or deleted, and cut where an edit rewrote the
//! lines it covered. What a caller wants of a cut position differs, which
//! is why there is more than one projection here (gerrit's
//! `GitPositionTransformer` and its conflict strategies).

use std::ops::Range;

use imara_diff::InternedInput;

use super::diff;

/// A 0-based, half-open line range; `Edit` turns one into another.
///
/// [`imara_diff::Hunk`]'s `before`/`after`, which is already what the
/// projection speaks. A pure insertion has an empty `before`, a pure
/// deletion an empty `after`.
pub type Span = Range<u32>;
pub type Edit = imara_diff::Hunk;

/// One edit per contiguous change region of `old → new`.
pub fn buffer_edits(old: &[u8], new: &[u8]) -> Vec<Edit> {
    diff::line_edits(&InternedInput::new(old, new))
}

fn net_delta(e: &Edit) -> i64 {
    i64::from(e.after.end - e.after.start) - i64::from(e.before.end - e.before.start)
}

/// Maps the parts of `pos` the mappings missed into B coordinates.
///
/// The parts of `pos` that the edits (`mappings`) did **not** touch move
/// into the mappings' B-coordinate space, each surviving sub-range shifted
/// by the running insert/delete delta of the mappings before it. A part of
/// `pos` covered by a mapping's A-range is dropped (gerrit's
/// `OmitPositionOnConflict`, refined to line granularity: a mapping that
/// straddles one end of `pos` still contributes the lines outside it).
///
/// `mappings` must be ascending by `before.start` and disjoint —
/// `buffer_edits` (one edit per ascending hunk) yields them that way.
pub fn project_clipped(pos: &Span, mappings: &[Edit]) -> Vec<Span> {
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

/// Maps `pos` whole into B coordinates, or `None` when a mapping touched it.
///
/// The position keeps its length and moves by the net delta of the
/// mappings above it. A mapping that covers, cuts or splits it means the
/// lines it named were rewritten, and no span in B is those lines
/// (gerrit's range conflict). A mapping that ends exactly where the
/// position starts, or starts where it ends, only shifts it.
///
/// `mappings` must be ascending by `before.start` and disjoint, as for
/// [`project_clipped`].
pub fn shift(pos: &Span, mappings: &[Edit]) -> Option<Span> {
    match project_clipped(pos, mappings).as_slice() {
        [whole] if whole.len() == pos.len() => Some(whole.clone()),
        _ => None,
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn span(start: u32, end: u32) -> Span {
        start..end
    }

    pub fn edit(before: (u32, u32), after: (u32, u32)) -> Edit {
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
        // A position inside the edit is dropped.
        assert!(project_clipped(&span(6, 7), &[edit((5, 8), (5, 8))]).is_empty());
    }

    #[test]
    fn project_clipped_keeps_the_part_outside_the_edit() {
        // The mapping straddles one end of the position, and the untouched
        // remainder still projects (size-neutral mapping ⇒ no shift).
        let m = [edit((5, 8), (5, 8))];
        assert_eq!(project_clipped(&span(4, 6), &m), vec![span(4, 5)]);
        assert_eq!(project_clipped(&span(7, 9), &m), vec![span(8, 9)]);
        // An interior edit splits the position in two.
        assert_eq!(
            project_clipped(&span(1, 9), &[edit((4, 5), (4, 5))]),
            vec![span(1, 4), span(5, 9)]
        );
    }

    #[test]
    fn shift_moves_a_whole_position_and_refuses_a_cut_one() {
        // Two lines inserted above move it down; a deletion above moves it up.
        assert_eq!(
            shift(&span(5, 7), &[edit((0, 0), (0, 2))]),
            Some(span(7, 9))
        );
        assert_eq!(
            shift(&span(8, 9), &[edit((5, 8), (5, 5))]),
            Some(span(5, 6))
        );
        // An insertion at either end of the position touches none of its
        // lines.
        assert_eq!(
            shift(&span(5, 7), &[edit((5, 5), (5, 6))]),
            Some(span(6, 8))
        );
        assert_eq!(
            shift(&span(5, 7), &[edit((7, 7), (7, 8))]),
            Some(span(5, 7))
        );
        // Covered, cut at one end, or split in the middle: the lines are gone.
        assert_eq!(shift(&span(5, 7), &[edit((4, 8), (4, 8))]), None);
        assert_eq!(shift(&span(5, 7), &[edit((6, 8), (6, 8))]), None);
        assert_eq!(shift(&span(5, 7), &[edit((6, 6), (6, 7))]), None);
    }
}
