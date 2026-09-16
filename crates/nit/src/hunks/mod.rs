//! Line-level diffing: the hunks of one file's two versions.
//!
//! Text in, hunks out. The api layer owns the git objects and the wire
//! shapes around them; this module owns which lines a diff compares and
//! where the hunk boundaries fall between them.

mod outline;

use std::ops::Range;

use imara_diff::{Algorithm, InternedInput};

use nit_types::diff::{Hunk, Line};
use nit_types::domain::{DiffMode, LineKind, Side};

use outline::outline;

/// The hunks of `old → new`, each with `context` unchanged lines around it.
///
/// Under [`DiffMode::Outline`] both sides are collapsed before they are
/// diffed, so the hunks describe the change to the file's outline. `path`
/// names the file, which is what picks the grammar the collapse parses it
/// with.
#[must_use]
pub fn of_file(path: &str, old: &str, new: &str, context: u32, mode: DiffMode) -> Vec<Hunk> {
    match mode {
        DiffMode::Full => line_hunks(&InternedInput::new(old, new), context, &Lines::Every),
        DiffMode::Outline => {
            let (before, old) = outline(path, old);
            let (after, new) = outline(path, new);
            let mut input = InternedInput::default();
            input.update_before(old.into_iter());
            input.update_after(new.into_iter());
            line_hunks(&input, context, &Lines::Kept { before, after })
        }
    }
}

/// The line diff `old → new`, as the ranges of changed lines on each side.
///
/// Histogram, gerrit's algorithm (`JGit`'s `HistogramDiff`): it anchors on the
/// rarest line the two sides share, so a unique signature pins the alignment
/// where myers — which only minimises the edit script — pairs whatever lies
/// nearest and steals a brace from the neighbouring block. `postprocess_lines`
/// then applies git's indent heuristic to the hunks whose placement is still
/// ambiguous.
#[must_use]
pub fn line_edits<T: AsRef<[u8]>>(input: &InternedInput<T>) -> Vec<imara_diff::Hunk> {
    let mut diff = imara_diff::Diff::compute(Algorithm::Histogram, input);
    diff.postprocess_lines(input);
    diff.hunks().collect()
}

/// Which of a file's lines reached the diff, and where they sit in it.
///
/// A full diff reads every line, so a line's index in it is its line in the
/// file. An outline diff reads only the lines its collapse kept, so the
/// numbers it reports have to come back out of the file they were taken
/// from — anything else would anchor a comment to a line that is not the
/// one shown.
enum Lines {
    Every,
    Kept { before: Vec<u64>, after: Vec<u64> },
}

impl Lines {
    /// The 1-based file line the before/after side's `index` was read from.
    fn at(&self, side: Side, index: usize) -> u64 {
        match self {
            Self::Every => index as u64 + 1,
            Self::Kept { before, after } => match side {
                Side::Old => before[index],
                Side::New => after[index],
            },
        }
    }
}

/// The wire hunks of `old → new`.
///
/// Each run of changed lines with `context` unchanged lines on either
/// side, runs closer than twice that merged into one hunk (git's
/// grouping, so a hunk never shows the same line twice).
///
/// A hunk is always consecutive on each side, so a body an outline
/// collapsed falls *between* two hunks — the same shape as context the
/// diff does not show, which the client already counts and expands.
fn line_hunks(input: &InternedInput<&str>, context: u32, lines: &Lines) -> Vec<Hunk> {
    let edits: Vec<(Range<usize>, Range<usize>)> = line_edits(input)
        .into_iter()
        .map(|h| (range(h.before), range(h.after)))
        .collect();
    // The tokens carry their line separator; the wire text never does.
    let text = |token| {
        let line: &str = input.interner[token];
        line.strip_suffix('\n').unwrap_or(line)
    };
    let ctx = context as usize;

    // The header is git's default rule: the nearest line above the hunk whose
    // first character is alphabetic, `_` or `$` (no support for the
    // per-language `diff` drivers a `.gitattributes` can name). Hunks ascend,
    // so a cursor that only moves forward reads the file once — searching
    // backwards would re-read it per hunk on one with no declaration at all.
    let (mut scanned, mut header) = (0usize, "");
    let mut header_above = |line: usize| {
        for token in &input.before[scanned..line] {
            let text: &str = input.interner[*token];
            if text.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$') {
                header = text.trim_end();
            }
        }
        scanned = line;
        header.to_string()
    };

    let context_upto = |out: &mut Vec<(Line, usize)>, b: &mut usize, a: &mut usize, upto: usize| {
        while *b < upto {
            let line = wire_line(
                LineKind::Context,
                Some(lines.at(Side::Old, *b)),
                Some(lines.at(Side::New, *a)),
                text(input.before[*b]),
            );
            out.push((line, *b));
            *b += 1;
            *a += 1;
        }
    };

    let mut hunks = Vec::new();
    for group in edits.chunk_by(|a, b| b.0.start - a.0.end <= 2 * ctx) {
        let (first, last) = (&group[0], &group[group.len() - 1]);
        // Both sides are identical outside the group, so one reach bounds
        // both: before the first edit their line numbers agree.
        let back = ctx.min(first.0.start);
        let before_end = (last.0.end + ctx).min(input.before.len());
        let (before_start, after_start) = (first.0.start - back, first.1.start - back);
        let (mut b, mut a) = (before_start, after_start);

        // Each line with the before-index it was read at, which is where its
        // hunk's header is searched from.
        let mut emitted: Vec<(Line, usize)> = Vec::new();
        for (before, after) in group {
            context_upto(&mut emitted, &mut b, &mut a, before.start);
            for i in before.clone() {
                let line = wire_line(
                    LineKind::Del,
                    Some(lines.at(Side::Old, i)),
                    None,
                    text(input.before[i]),
                );
                emitted.push((line, i));
            }
            for i in after.clone() {
                let line = wire_line(
                    LineKind::Add,
                    None,
                    Some(lines.at(Side::New, i)),
                    text(input.after[i]),
                );
                emitted.push((line, b));
            }
            (b, a) = (before.end, after.end);
        }
        context_upto(&mut emitted, &mut b, &mut a, before_end);
        hunks.extend(contiguous_hunks(emitted, &mut header_above));
    }
    hunks
}

/// Splits a group's lines into hunks that skip no file line.
///
/// A collapsed body is a run the lines step over, and stepping over a run is
/// what a hunk boundary already means — so the diff carries it as the gap
/// between two hunks rather than as anything new. A piece left with no
/// change of its own is context the collapse stranded, and is dropped.
fn contiguous_hunks(
    emitted: Vec<(Line, usize)>,
    header_above: &mut impl FnMut(usize) -> String,
) -> Vec<Hunk> {
    let steps_over = |last: Option<u64>, at: Option<u64>| matches!((last, at), (Some(last), Some(at)) if at > last + 1);
    let mut hunks: Vec<Hunk> = Vec::new();
    let (mut piece, mut piece_at) = (Vec::new(), 0);
    let (mut last_old, mut last_new) = (None, None);
    let (mut before_old, mut before_new) = (0, 0);

    let mut close = |piece: &mut Vec<Line>, at, before_old, before_new| {
        if !piece.iter().any(|l: &Line| l.kind != LineKind::Context) {
            piece.clear();
            return;
        }
        // Contiguous, so a side's span is however many of its lines are here.
        let extent = |numbers: Vec<u64>, sits_after| match numbers.first() {
            Some(first) => (*first, numbers.len() as u64),
            None => (sits_after, 0),
        };
        let (old_start, old_lines) =
            extent(piece.iter().filter_map(|l| l.old).collect(), before_old);
        let (new_start, new_lines) =
            extent(piece.iter().filter_map(|l| l.new).collect(), before_new);
        hunks.push(Hunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            header: header_above(at),
            lines: std::mem::take(piece),
        });
    };

    for (line, at) in emitted {
        if steps_over(last_old, line.old) || steps_over(last_new, line.new) {
            close(&mut piece, piece_at, before_old, before_new);
            (before_old, before_new) = (last_old.unwrap_or(0), last_new.unwrap_or(0));
            piece_at = at;
        }
        if piece.is_empty() {
            piece_at = at;
        }
        last_old = line.old.or(last_old);
        last_new = line.new.or(last_new);
        piece.push(line);
    }
    close(&mut piece, piece_at, before_old, before_new);
    hunks
}

fn range(r: Range<u32>) -> Range<usize> {
    r.start as usize..r.end as usize
}

#[must_use]
pub fn wire_line(kind: LineKind, old: Option<u64>, new: Option<u64>, text: &str) -> Line {
    Line {
        kind,
        old,
        new,
        drift: false,
        text: text.to_string(),
    }
}
