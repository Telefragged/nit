//! Line-level diffing: the hunks of one file's two versions.
//!
//! Text in, hunks out. The api layer owns the git objects and the wire
//! shapes around them; this module owns which lines a diff compares and
//! where the hunk boundaries fall between them.

use std::ops::Range;

use imara_diff::{Algorithm, InternedInput};

use nit_types::diff::{Hunk, Line};
use nit_types::domain::{LineKind, Whitespace};

/// The hunks of `old → new`, each with `context` unchanged lines around it.
#[must_use]
pub fn of_file(old: &str, new: &str, context: u32, whitespace: Whitespace) -> Vec<Hunk> {
    let before: Vec<&str> = old.split_inclusive('\n').collect();
    let after: Vec<&str> = new.split_inclusive('\n').collect();
    line_hunks(&before, &after, context, whitespace)
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

/// The text a line is compared by: the line itself, or the line with its
/// whitespace removed.
fn compared(line: &str, whitespace: Whitespace) -> String {
    match whitespace {
        Whitespace::Compare => line.to_string(),
        Whitespace::Ignore => line.split_whitespace().collect(),
    }
}

/// The wire hunks of `old → new`.
///
/// Each run of changed lines with `context` unchanged lines on either
/// side, runs closer than twice that merged into one hunk (git's
/// grouping, so a hunk never shows the same line twice).
fn line_hunks(before: &[&str], after: &[&str], context: u32, whitespace: Whitespace) -> Vec<Hunk> {
    let mut input = InternedInput::default();
    input.update_before(before.iter().map(|line| compared(line, whitespace)));
    input.update_after(after.iter().map(|line| compared(line, whitespace)));
    let edits: Vec<(Range<usize>, Range<usize>)> = line_edits(&input)
        .into_iter()
        .map(|h| (range(h.before), range(h.after)))
        .collect();
    // The lines carry their separator; the wire text never does.
    let text = |line: &str| line.strip_suffix('\n').unwrap_or(line).to_string();
    let number = |index: usize| Some(index as u64 + 1);
    let ctx = context as usize;

    // The header is git's default rule: the nearest line above the hunk whose
    // first character is alphabetic, `_` or `$` (no support for the
    // per-language `diff` drivers a `.gitattributes` can name). Hunks ascend,
    // so a cursor that only moves forward reads the file once — searching
    // backwards would re-read it per hunk on one with no declaration at all.
    let (mut scanned, mut header) = (0usize, "");
    let mut header_above = |line: usize| {
        for read in &before[scanned..line] {
            if read.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$') {
                header = read.trim_end();
            }
        }
        scanned = line;
        header.to_string()
    };

    let context_upto = |out: &mut Vec<Line>, b: &mut usize, a: &mut usize, upto: usize| {
        while *b < upto {
            // Under `Whitespace::Ignore` a line the change only re-indented
            // is context, and its two sides differ. The reviewer reads the
            // file as the change leaves it, so the text shown is the new
            // side's.
            out.push(wire_line(
                LineKind::Context,
                number(*b),
                number(*a),
                text(after[*a]),
            ));
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
        let before_end = (last.0.end + ctx).min(before.len());
        let (before_start, after_start) = (first.0.start - back, first.1.start - back);
        let (mut b, mut a) = (before_start, after_start);

        let mut lines = Vec::new();
        for (dels, adds) in group {
            context_upto(&mut lines, &mut b, &mut a, dels.start);
            for i in dels.clone() {
                lines.push(wire_line(LineKind::Del, number(i), None, text(before[i])));
            }
            for i in adds.clone() {
                lines.push(wire_line(LineKind::Add, None, number(i), text(after[i])));
            }
            (b, a) = (dels.end, adds.end);
        }
        context_upto(&mut lines, &mut b, &mut a, before_end);
        // A side with no lines starts at the line above the hunk (git's `-N,0`).
        let start = |from: usize, to: usize| {
            if to > from {
                from as u64 + 1
            } else {
                from as u64
            }
        };
        hunks.push(Hunk {
            old_start: start(before_start, b),
            old_lines: (b - before_start) as u64,
            new_start: start(after_start, a),
            new_lines: (a - after_start) as u64,
            header: header_above(before_start),
            lines,
        });
    }
    hunks
}

fn range(r: Range<u32>) -> Range<usize> {
    r.start as usize..r.end as usize
}

#[must_use]
pub fn wire_line(kind: LineKind, old: Option<u64>, new: Option<u64>, text: String) -> Line {
    Line {
        kind,
        old,
        new,
        drift: false,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEFORE: &str = "fn a() {\n    one();\n    two();\n}\n";

    #[test]
    fn a_re_indented_body_does_not_reach_the_diff() {
        let after = "fn a() {\n        one();\n        two();\n}\n";
        assert_eq!(
            of_file(BEFORE, after, 3, Whitespace::Compare).len(),
            1,
            "compared whole, the two indentations differ"
        );
        assert!(of_file(BEFORE, after, 3, Whitespace::Ignore).is_empty());
    }

    #[test]
    fn an_ignored_line_shows_the_new_side() {
        // Line 2 is re-indented alone; line 3 is really edited.
        let after = "fn a() {\n        one();\n    three();\n}\n";
        let hunks = of_file(BEFORE, after, 3, Whitespace::Ignore);

        let kinds: Vec<_> = hunks[0]
            .lines
            .iter()
            .map(|l| (l.kind, l.text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (LineKind::Context, "fn a() {"),
                (LineKind::Context, "        one();"),
                (LineKind::Del, "    two();"),
                (LineKind::Add, "    three();"),
                (LineKind::Context, "}"),
            ]
        );
    }
}
