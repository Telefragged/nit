//! The source lines that a line comment anchors to, quoted under it.

use std::collections::HashMap;

use annotate_snippets::renderer::{DEFAULT_TERM_WIDTH, DecorStyle};
use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
use anyhow::Result;

use nit_types::diff::{FileLines, Line};
use nit_types::domain::{
    Anchor, ChangeNumber, CommentInput, LineAnchor, LogEntry, LogPayload, RevisionNumber, Side,
};

use super::client::{Client, Retry};

/// The lines of context above and below the anchored lines.
const CONTEXT: u64 = 2;

/// The columns that the log indents a quote by, at most.
const INDENT: usize = 8;

/// One file at one revision of a change.
type FileKey = (ChangeNumber, RevisionNumber, String);

/// The lines of each file that the log's line comments anchor to.
#[cfg_attr(test, derive(Default))]
pub(crate) struct Sources {
    files: HashMap<FileKey, Vec<Line>>,
    /// The columns that a quote may fill.
    width: usize,
}

impl Sources {
    /// Fetches each file that a line comment in `entries` anchors to, once.
    ///
    /// The lines come from the server, not from the local checkout, so the
    /// quote works when the server runs on another machine. The quotes
    /// fit the width of the terminal on stdout, or annotate-snippets'
    /// default width when stdout is not a terminal.
    ///
    /// # Errors
    ///
    /// When the server rejects a request, or `retry` gives up on reaching
    /// it.
    pub(crate) fn fetch(client: &Client, entries: &[LogEntry], retry: Retry) -> Result<Sources> {
        let mut files = HashMap::new();
        for entry in entries {
            let comments = match &entry.payload {
                LogPayload::Review(p) => &p.comments[..],
                LogPayload::Comment(c) => std::slice::from_ref(c),
                _ => &[],
            };
            for comment in comments {
                let Some((key, ..)) = anchored(entry.change_number, comment) else {
                    continue;
                };
                if files.contains_key(&key) {
                    continue;
                }
                let (change, revision, file) = &key;
                let query = serde_html_form::to_string([("path", file)])
                    .expect("a query of one string serializes");
                let lines: FileLines = client.get_retry(
                    &format!("/api/changes/{change}/revisions/{revision}/lines?{query}"),
                    retry,
                )?;
                files.insert(key, lines.lines);
            }
        }
        let terminal = terminal_size::terminal_size_of(std::io::stdout())
            .map_or(DEFAULT_TERM_WIDTH, |(width, _)| usize::from(width.0));
        Ok(Sources {
            files,
            width: terminal.saturating_sub(INDENT),
        })
    }

    /// The anchored lines of `comment`, quoted with their context and the
    /// comment's body.
    ///
    /// `None` when the comment anchors to no line, or when its file or
    /// line is not in the sources.
    pub(crate) fn quote(&self, change: ChangeNumber, comment: &CommentInput) -> Option<String> {
        let (key, side, at) = anchored(change, comment)?;
        let lines = self.files.get(&key)?;
        quote(&key.2, side, at, lines, &comment.body, self.width)
    }
}

/// The file, side and place that a line comment anchors to.
fn anchored(change: ChangeNumber, comment: &CommentInput) -> Option<(FileKey, Side, LineAnchor)> {
    match (&comment.anchor, comment.revision) {
        (Some(Anchor::Line { file, side, at, .. }), Some(revision)) => {
            Some(((change, revision, file.clone()), *side, *at))
        }
        _ => None,
    }
}

/// Renders the lines of `side` that `at` covers, with [`CONTEXT`] lines
/// around them and `body` attached, all within `width` columns.
///
/// Each covered line gets its own underline, from its first non-blank
/// character unless the selection starts later. A one-line body that fits
/// after the last underline labels it, as a rustc label does. Any other
/// body goes under the quote, because a label breaks the gutter at each
/// line break.
///
/// `None` when the side does not hold the anchored lines.
fn quote(
    file: &str,
    side: Side,
    at: LineAnchor,
    lines: &[Line],
    body: &str,
    width: usize,
) -> Option<String> {
    let (start_line, end_line, start_char, end_char) = match at {
        LineAnchor::Whole(line) => (line, line, None, None),
        LineAnchor::Selection(r) => (
            r.start_line(),
            r.end_line(),
            Some(r.start_char()),
            Some(r.end_char()),
        ),
    };
    let first = start_line.saturating_sub(CONTEXT).max(1);
    let last = end_line + CONTEXT;
    let window: Vec<(u64, &str)> = lines
        .iter()
        .filter_map(|l| {
            let number = match side {
                Side::Old => l.old,
                Side::New => l.new,
            }?;
            (first..=last)
                .contains(&number)
                .then_some((number, l.text.as_str()))
        })
        .collect();
    if !window.iter().any(|(number, _)| *number == end_line) {
        return None;
    }

    let mut source = String::new();
    let mut spans = Vec::new();
    // The column that the last underline ends on, in characters.
    let mut end_column = 0;
    for (number, text) in &window {
        if (start_line..=end_line).contains(number) {
            let byte = |char: u64| {
                usize::try_from(char)
                    .ok()
                    .and_then(|char| text.char_indices().nth(char))
                    .map_or(text.len(), |(i, _)| i)
            };
            let start = match start_char {
                Some(char) if *number == start_line => byte(char),
                _ => text.len() - text.trim_start().len(),
            };
            let end = match end_char {
                Some(char) if *number == end_line => byte(char),
                _ => text.len(),
            };
            // A blank line inside a selection has nothing to underline.
            if start < end || start_line == end_line {
                spans.push(source.len() + start..source.len() + end);
                end_column = text[..end].chars().count();
            }
        }
        source.push_str(text);
        source.push('\n');
    }

    let last_span = spans.len().checked_sub(1)?;
    // The gutter holds the widest line number, then ` │ `.
    let gutter = window.last()?.0.to_string().len() + 3;
    let fits = gutter + end_column + 1 + body.chars().count() <= width;
    let label = (fits && !body.contains('\n')).then_some(body);
    let annotations = spans.into_iter().enumerate().map(|(i, span)| {
        let underline = AnnotationKind::Primary.span(span);
        if i == last_span {
            underline.label(label)
        } else {
            underline
        }
    });
    let snippet = Snippet::source(&source)
        .line_start(usize::try_from(window[0].0).ok()?)
        .path(file)
        .fold(false)
        .annotations(annotations);
    let mut group = Group::with_level(Level::NOTE.no_name()).element(snippet);
    if label.is_none() {
        group = group.element(Level::NOTE.no_name().message(body));
    }
    Some(
        Renderer::plain()
            .decor_style(DecorStyle::Unicode)
            .term_width(width)
            .render(&[group]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::format::render_entries;
    use nit_types::domain::{CommentRange, LineKind, ReviewPayload, Verdict};

    /// A file of context lines, numbered alike on both sides.
    fn file(texts: &[&str]) -> Vec<Line> {
        texts
            .iter()
            .zip(1..)
            .map(|(text, n)| Line {
                kind: LineKind::Context,
                old: Some(n),
                new: Some(n),
                drift: false,
                text: (*text).to_string(),
            })
            .collect()
    }

    fn comment(tid: u64, at: LineAnchor, body: &str) -> CommentInput {
        CommentInput {
            thread_id: Some(tid),
            revision: Some(RevisionNumber::new(1)),
            anchor: Some(
                Anchor::parse(Some("src/queue.rs".into()), Some(Side::New), Some(at))
                    .expect("a fixture names one anchor"),
            ),
            body: body.to_string(),
            resolved: None,
        }
    }

    #[test]
    fn review_quotes_the_anchored_lines() {
        let change = ChangeNumber::new(7);
        let lines = file(&[
            "fn main() {",
            "    let (tx, rx) = channel(0);",
            "    run(tx);",
            "}",
            "",
            "fn run() {}",
        ]);
        let sources = Sources {
            files: HashMap::from([(
                (change, RevisionNumber::new(1), "src/queue.rs".to_string()),
                lines,
            )]),
            width: 60,
        };
        let range = CommentRange::new(2, 8, 3, 7).expect("a forward range");
        let entry = LogEntry {
            change_number: change,
            position: 0,
            sequence: 4,
            created_at: String::new(),
            payload: LogPayload::Review(ReviewPayload {
                revision: RevisionNumber::new(1),
                verdict: Verdict::RequestChanges,
                message: String::new(),
                comments: vec![
                    comment(0, LineAnchor::Whole(2), "Bounded channel."),
                    comment(
                        1,
                        LineAnchor::Selection(range),
                        "Spans two lines.\nSecond line.",
                    ),
                    comment(
                        2,
                        LineAnchor::Whole(3),
                        "A one-line body too long to fit after the underline.",
                    ),
                    comment(3, LineAnchor::Whole(40), "Past the end."),
                ],
            }),
        };
        // An underline starts at the first non-blank unless the selection
        // starts later. A one-line body that fits labels the last
        // underline, and any other body goes under the quote. A line the
        // file does not hold keeps its anchor label.
        assert_eq!(
            render_entries(&[entry], &sources),
            "sequence 4  change 7 r1  reviewer: request_changes\n\
             \x20   t0\n\
             \x20         ╭▸ src/queue.rs:2:5\n\
             \x20         │\n\
             \x20       1 │ fn main() {\n\
             \x20       2 │     let (tx, rx) = channel(0);\n\
             \x20         │     ━━━━━━━━━━━━━━━━━━━━━━━━━━ Bounded channel.\n\
             \x20       3 │     run(tx);\n\
             \x20       4 │ }\n\
             \x20         ╰╴\n\
             \x20   t1\n\
             \x20         ╭▸ src/queue.rs:2:9\n\
             \x20         │\n\
             \x20       1 │ fn main() {\n\
             \x20       2 │     let (tx, rx) = channel(0);\n\
             \x20         │         ━━━━━━━━━━━━━━━━━━━━━━\n\
             \x20       3 │     run(tx);\n\
             \x20         │     ━━━\n\
             \x20       4 │ }\n\
             \x20       5 │\n\
             \x20         │\n\
             \x20         ╰ Spans two lines.\n\
             \x20           Second line.\n\
             \x20   t2\n\
             \x20         ╭▸ src/queue.rs:3:5\n\
             \x20         │\n\
             \x20       1 │ fn main() {\n\
             \x20       2 │     let (tx, rx) = channel(0);\n\
             \x20       3 │     run(tx);\n\
             \x20         │     ━━━━━━━━\n\
             \x20       4 │ }\n\
             \x20       5 │\n\
             \x20         │\n\
             \x20         ╰ A one-line body too long to fit after the underline.\n\
             \x20   t3  src/queue.rs:40\n\
             \x20       Past the end."
        );
    }
}
