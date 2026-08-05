//! Intraline emphasis: the changed characters of a replacement block.
//!
//! The block diffs as a single text — the deleted lines joined, the added
//! lines joined — so content that moved between lines still pairs up, and a
//! block whose two sides differ in length has no leftovers. A per-character
//! diff on its own leaves marks too scattered to read, so two of Gerrit's
//! `IntraLineLoader` rules follow it: near edits join, and matching edges go
//! back to the context. Its slider is deliberately not among them: shifting
//! one side's span alone does not preserve the transformation, so the marks
//! would stop describing the change.
//!
//! Offsets are UTF-16 code units throughout — how the browser indexes the
//! strings it sends and slices the ranges it gets back.

use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};
use serde::{Deserialize, Serialize};
use std::iter::Copied;
use std::ops::Range;
use std::slice;

const NL: u16 = b'\n' as u16;

/// Code units a side may hold before its block goes unmarked.
///
/// Myers is O(N·D), and repetitive text — a generated file, a lockfile, a
/// long run of near-identical lines — offers no unique anchor to cut the
/// search grid on: 10k units a side costs ~20ms compiled, 38k ~270ms, 77k
/// over a second, and wasm is slower again. Ordinary prose and code stay in
/// the low milliseconds at any size, but the budget has to hold for the
/// worst shape, and a block this large is a wholesale rewrite its add/del
/// tint already conveys.
const BUDGET: usize = 10_000;

/// A replacement block: the deleted lines, and the added lines that
/// replace them.
#[derive(Deserialize)]
pub struct Region {
    old: Vec<String>,
    new: Vec<String>,
}

/// Character ranges to emphasize.
///
/// One list per line of the [`Region`] they came from, in the same order.
#[derive(Serialize)]
pub struct Marks {
    old: Vec<Vec<(usize, usize)>>,
    new: Vec<Vec<(usize, usize)>>,
}

/// Marks the changed characters of every region.
pub fn marks(regions: &[Region]) -> Vec<Marks> {
    regions
        .iter()
        .map(|region| {
            let (old, new) = (Side::new(&region.old), Side::new(&region.new));
            let edits = edits(&old.units, &new.units);
            Marks {
                old: old.project(edits.iter().map(|e| &e.old)),
                new: new.project(edits.iter().map(|e| &e.new)),
            }
        })
        .collect()
}

/// One side of a region: its lines run together, newline-terminated.
///
/// Each line's place in the run is recorded alongside it. Terminating the
/// last line too lets an edit reach its end the way it reaches any other's.
struct Side {
    units: Vec<u16>,
    lines: Vec<Range<usize>>,
}

impl Side {
    fn new(lines: &[String]) -> Self {
        let mut units = Vec::new();
        let lines = lines
            .iter()
            .map(|line| {
                let start = units.len();
                units.extend(line.encode_utf16());
                let span = start..units.len();
                units.push(NL);
                span
            })
            .collect();
        Self { units, lines }
    }

    /// Cuts the edit spans up by line.
    ///
    /// Clamping drops the newlines an edit swallowed, so every mark ends
    /// where its line's text does. A mark over a whole line is dropped: the
    /// line already renders as added or deleted, and emphasising all of it —
    /// indentation included — says nothing more.
    fn project<'a>(
        &self,
        spans: impl Iterator<Item = &'a Range<usize>>,
    ) -> Vec<Vec<(usize, usize)>> {
        let spans: Vec<_> = spans.collect();
        self.lines
            .iter()
            .map(|line| {
                spans
                    .iter()
                    .filter_map(|span| {
                        let from = span.start.clamp(line.start, line.end);
                        let to = span.end.clamp(line.start, line.end);
                        if from >= to {
                            return None;
                        }
                        // A boundary between the halves of a surrogate pair
                        // cuts a code point in two when the browser slices the
                        // line, and each half renders as U+FFFD. Widen to the
                        // whole pair; a line never starts or ends mid-pair, so
                        // the mark stays inside it.
                        let from = from - usize::from(is_low_surrogate(self.units[from]));
                        let to = to + usize::from(is_low_surrogate(self.units[to]));
                        let whole = from == line.start && to == line.end;
                        (!whole).then_some((from - line.start, to - line.start))
                    })
                    .collect()
            })
            .collect()
    }
}

/// The trailing half of a UTF-16 surrogate pair.
const fn is_low_surrogate(unit: u16) -> bool {
    matches!(unit, 0xDC00..=0xDFFF)
}

/// The old span an edit replaces, and the new span it puts there.
///
/// Either may be empty: an insertion has no old span, a deletion no new one.
struct Edit {
    old: Range<usize>,
    new: Range<usize>,
}

/// The code units of one side, as diff tokens.
struct Units<'a>(&'a [u16]);

impl<'a> TokenSource for Units<'a> {
    type Token = u16;
    type Tokenizer = Copied<slice::Iter<'a, u16>>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.0.iter().copied()
    }

    fn estimate_tokens(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap_or(u32::MAX)
    }
}

fn edits(old: &[u16], new: &[u16]) -> Vec<Edit> {
    if old.len() > BUDGET || new.len() > BUDGET {
        return Vec::new();
    }
    let input = InternedInput::new(Units(old), Units(new));
    let mut edits: Vec<Edit> = Diff::compute(Algorithm::Myers, &input)
        .hunks()
        .map(|hunk| Edit {
            old: hunk.before.start as usize..hunk.before.end as usize,
            new: hunk.after.start as usize..hunk.after.end as usize,
        })
        .collect();
    coalesce(&mut edits, old, new);
    for edit in &mut edits {
        trim(edit, old, new);
    }
    edits
}

/// Joins edits a handful of characters apart.
///
/// Over so short a gap, two marks with a sliver of context between them
/// read worse than one covering both. A gap crossing a line break is left
/// alone — those marks land on different lines.
fn coalesce(edits: &mut Vec<Edit>, old: &[u16], new: &[u16]) {
    let mut at = 0;
    while at + 1 < edits.len() {
        let (gap_old, gap_new) = (
            edits[at].old.end..edits[at + 1].old.start,
            edits[at].new.end..edits[at + 1].new.start,
        );
        let near = gap_old.len() <= 5 || gap_new.len() <= 5;
        if near && !old[gap_old].contains(&NL) && !new[gap_new].contains(&NL) {
            let next = edits.remove(at + 1);
            edits[at].old.end = next.old.end;
            edits[at].new.end = next.new.end;
            continue;
        }
        at += 1;
    }
}

/// Gives matching text at an edit's own edges back to the context.
///
/// Coalescing can leave it inside the edit; handing it back makes a mark
/// start and end where the texts part.
fn trim(edit: &mut Edit, old: &[u16], new: &[u16]) {
    while !edit.old.is_empty() && !edit.new.is_empty() && old[edit.old.start] == new[edit.new.start]
    {
        edit.old.start += 1;
        edit.new.start += 1;
    }
    while !edit.old.is_empty()
        && !edit.new.is_empty()
        && old[edit.old.end - 1] == new[edit.new.end - 1]
    {
        edit.old.end -= 1;
        edit.new.end -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line's marked fragments, `|`-separated — an expectation reads as
    /// the text a reviewer sees emphasized.
    fn marked(old: &[&str], new: &[&str]) -> (Vec<String>, Vec<String>) {
        let region = Region {
            old: old.iter().map(|s| (*s).to_string()).collect(),
            new: new.iter().map(|s| (*s).to_string()).collect(),
        };
        let out = marks(slice::from_ref(&region));
        let cut = |lines: &[&str], marks: &[Vec<(usize, usize)>]| {
            lines
                .iter()
                .zip(marks)
                .map(|(line, spans)| {
                    let units: Vec<u16> = line.encode_utf16().collect();
                    spans
                        .iter()
                        .map(|&(from, to)| String::from_utf16_lossy(&units[from..to]))
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .collect()
        };
        (cut(old, &out[0].old), cut(new, &out[0].new))
    }

    #[test]
    fn marks_the_changed_word_and_nothing_around_it() {
        let (old, new) = marked(&["let x = old_value;"], &["let x = new_value;"]);
        assert_eq!(old, ["old"]);
        assert_eq!(new, ["new"]);
    }

    #[test]
    fn marks_each_changed_run_of_a_line_separately() {
        let (old, new) = marked(
            &["fn f(a: u8, b: u8) -> u8"],
            &["fn f(a: u32, b: u32) -> u8"],
        );
        assert_eq!(old, ["8|8"]);
        assert_eq!(new, ["32|32"]);
    }

    #[test]
    fn pairs_content_that_moved_to_another_line() {
        // One deleted line against two added ones: nothing pairs positionally,
        // yet `compute(x, y)` has to come out unmarked on the line it moved to.
        let (old, new) = marked(
            &["out = compute(x, y) + 1;"],
            &["temp = compute(x, y);", "out = temp + 1;"],
        );
        assert_eq!(old, ["out"]);
        assert_eq!(new, ["temp|;", "out = temp"]);
    }

    #[test]
    fn leaves_a_wholly_rewritten_line_to_its_own_tint() {
        let (old, new) = marked(&["abcdefghij"], &["0123456789"]);
        assert_eq!(old, [""]);
        assert_eq!(new, [""]);
    }

    #[test]
    fn joins_edits_a_few_characters_apart() {
        let (_, new) = marked(&["const n = 1 + 2;"], &["const n = 3 + 4;"]);
        assert_eq!(new, ["3 + 4"]);
    }

    #[test]
    fn leaves_an_unchanged_line_of_the_block_unmarked() {
        let (old, new) = marked(&["keep me", "drop me"], &["keep me", "take me"]);
        assert_eq!(old, ["", "drop"]);
        assert_eq!(new, ["", "take"]);
    }

    #[test]
    fn keeps_a_mark_off_the_middle_of_a_surrogate_pair() {
        // The two emoji share a high surrogate, so the code-unit diff parts
        // them one unit in — a mark starting there would halve the pair.
        let (old, new) = marked(&["x = 🎉;"], &["x = 🎈;"]);
        assert_eq!(old, ["🎉"]);
        assert_eq!(new, ["🎈"]);
    }

    #[test]
    fn leaves_a_block_past_the_search_budget_unmarked() {
        let line = "let value = compute(alpha, beta, gamma, delta);";
        let lines: Vec<String> = std::iter::repeat_n(line.to_string(), 400).collect();
        let region = Region {
            old: lines.clone(),
            new: lines
                .iter()
                .map(|l| l.replace("compute", "derive"))
                .collect(),
        };
        let out = marks(slice::from_ref(&region));
        assert!(out[0].new.iter().all(Vec::is_empty));
    }

    #[test]
    fn counts_offsets_in_utf16_code_units() {
        let region = Region {
            old: vec!["// 🎉 x = 1".to_string()],
            new: vec!["// 🎉 x = 2".to_string()],
        };
        // The emoji is one char but two code units: counting chars would put
        // the mark at 9.
        assert_eq!(marks(slice::from_ref(&region))[0].new, [[(10, 11)]]);
    }
}
