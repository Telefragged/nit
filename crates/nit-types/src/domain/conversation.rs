//! The review conversation: threads, where they anchor, and the
//! reviewer's unpublished drafts.

use serde::{Deserialize, Serialize};

use super::ChangeNumber;
use super::CommentInput;
use super::Decision;
use super::RevisionNumber;

/// Which tree of a revision a line comment is anchored to.
///
/// `new` is the revision's commit tree, `old` its parent tree. An
/// unspecified side is `new`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Old,
    #[default]
    New,
}

impl Side {
    /// The persisted/wire spelling — the `drafts.side` column value
    /// (db↔domain boundary).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Old => "old",
            Side::New => "new",
        }
    }
}

impl std::str::FromStr for Side {
    type Err = String;

    fn from_str(s: &str) -> Result<Side, String> {
        match s {
            "old" => Ok(Side::Old),
            "new" => Ok(Side::New),
            other => Err(format!(
                "invalid side {other:?} (expected \"old\" or \"new\")"
            )),
        }
    }
}

/// Where a thread is anchored within a revision.
///
/// Modeled so the invalid combinations the flat wire fields allow are
/// unrepresentable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// The change as a whole (no file).
    Change,
    /// A whole file (no line).
    File { file: String },
    /// A line, optionally a sub-line `range` selection within it.
    Line {
        file: String,
        side: Side,
        line: u64,
        line_text: Option<String>,
        range: Option<CommentRange>,
    },
}

impl Anchor {
    /// The anchor a new thread is born with, taken from its opening comment.
    #[must_use]
    pub fn from_input(c: &CommentInput) -> Anchor {
        match (&c.file, c.line) {
            (Some(file), Some(line)) => Anchor::Line {
                file: file.clone(),
                side: c.side.unwrap_or_default(),
                line,
                line_text: c.line_text.clone(),
                range: c.range,
            },
            (Some(file), None) => Anchor::File { file: file.clone() },
            (None, _) => Anchor::Change,
        }
    }
}

/// A located, resolvable conversation.
///
/// Its anchor and birth come from its first comment; the `id` is
/// fold-assigned by creation order, never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ThreadProjection {
    pub id: u64,
    pub revision: RevisionNumber,
    pub anchor: Anchor,
    pub resolved: bool,
    pub comments: Vec<ThreadComment>,
    pub created_at: String,
    pub updated_at: String,
}

/// One message in a thread.
///
/// `review_id` is the review that published it, or `None` for an author's
/// own note — which is what distinguishes reviewer from author (the only
/// consumer derives the label from it).
#[derive(Debug, Clone, Serialize, Deserialize)]
// Shares the wire `ThreadComment` name but is a distinct type — only
// ever round-tripped through the wasm fold.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(rename = "ThreadCommentProjection")
)]
pub struct ThreadComment {
    pub body: String,
    pub review_id: Option<u64>,
    pub created_at: String,
}

/// Selected-text anchor of a line comment.
///
/// 1-based lines on the comment's side, 0-based chars, `end_char`
/// exclusive, `end_line` = the comment's `line`. The JSON shape is these
/// four fields. They are domain coordinates (always non-negative), so the
/// shape is `u64`; the server's `SQLite` columns are signed, converted at
/// the db boundary like every other id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommentRange {
    pub start_line: u64,
    pub start_char: u64,
    pub end_line: u64,
    pub end_char: u64,
}

/// A reviewer's unpublished comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Draft {
    pub id: u64,
    pub change_number: ChangeNumber,
    pub thread_id: Option<u64>,
    /// The request's anchor revision; only a new thread uses it.
    pub revision: RevisionNumber,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: Side,
    pub range: Option<CommentRange>,
    pub line_text: Option<String>,
    /// May be empty for a resolution-only reply draft.
    pub body: String,
    /// The draft's thread-resolution decision (false when unset).
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A reviewer's draft decision plus its cover note/reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DraftDecision {
    pub decision: Decision,
    #[serde(default)]
    pub message: String,
}
