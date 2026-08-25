//! The review conversation: threads, where they anchor, and the
//! reviewer's unpublished drafts.

use serde::{Deserialize, Serialize};

use super::ChangeNumber;
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
    /// The wire spelling (mirrors the serde renaming).
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", from = "StoredAnchor")]
pub enum Anchor {
    /// The change as a whole (no file).
    Change,
    /// A whole file (no line).
    File { file: String },
    /// A place inside a file, on one side of the revision.
    Line {
        file: String,
        side: Side,
        line_text: Option<String>,
        at: LineAnchor,
    },
}

/// An anchor in either spelling, before it is an [`Anchor`].
///
/// The log is append-only, so an entry written before a line anchor held
/// one `at` keeps the `line` and `range` it was written with. Reading
/// resolves the two spellings into the same anchor.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredAnchor {
    Change,
    File {
        file: String,
    },
    Line {
        file: String,
        #[serde(default)]
        side: Side,
        #[serde(default)]
        line_text: Option<String>,
        #[serde(default)]
        at: Option<LineAnchor>,
        #[serde(default)]
        line: Option<u64>,
        #[serde(default)]
        range: Option<CommentRange>,
    },
}

impl From<StoredAnchor> for Anchor {
    fn from(stored: StoredAnchor) -> Anchor {
        match stored {
            StoredAnchor::Change => Anchor::Change,
            StoredAnchor::File { file } => Anchor::File { file },
            StoredAnchor::Line {
                file,
                side,
                line_text,
                at,
                line,
                range,
            } => {
                // The older spelling held the selection in `range`, and the
                // line it ends on in `line`.
                let at = at
                    .or_else(|| range.map(LineAnchor::Selection))
                    .or_else(|| line.map(LineAnchor::Whole));
                match at {
                    Some(at) => Anchor::Line {
                        file,
                        side,
                        line_text,
                        at,
                    },
                    None => Anchor::File { file },
                }
            }
        }
    }
}

/// Where inside a file a line anchor sits.
///
/// A selection ends on the line it anchors to, so both spellings name
/// exactly one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LineAnchor {
    /// The whole line.
    Whole(u64),
    /// A run of characters inside the line.
    Selection(CommentRange),
}

impl LineAnchor {
    /// The selection inside the line, if it names one.
    #[must_use]
    pub fn range(self) -> Option<CommentRange> {
        match self {
            LineAnchor::Whole(_) => None,
            LineAnchor::Selection(range) => Some(range),
        }
    }
}

impl Anchor {
    /// The anchor that a file, a line and a selection name together.
    ///
    /// A line names a place inside a file, so it needs one. Nothing else
    /// is an anchor, and the absent file is the change itself.
    ///
    /// # Errors
    ///
    /// [`AnchorError`], naming the rule the parts broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::{Anchor, LineAnchor};
    ///
    /// let at = LineAnchor::Whole(3);
    /// assert!(Anchor::parse(None, None, None).is_ok());
    /// assert!(Anchor::parse(None, None, Some(at)).is_err());
    /// assert!(Anchor::parse(Some("a.rs".into()), None, Some(at)).is_ok());
    /// ```
    pub fn parse(
        file: Option<String>,
        side: Option<Side>,
        at: Option<LineAnchor>,
    ) -> Result<Anchor, AnchorError> {
        match (file, at) {
            (Some(file), Some(at)) => Ok(Anchor::Line {
                file,
                side: side.unwrap_or_default(),
                line_text: None,
                at,
            }),
            (Some(file), None) => Ok(Anchor::File { file }),
            (None, None) => Ok(Anchor::Change),
            (None, Some(_)) => Err(AnchorError::LineWithoutFile),
        }
    }

    /// The file the anchor names, if it names one.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        match self {
            Anchor::Change => None,
            Anchor::File { file } | Anchor::Line { file, .. } => Some(file),
        }
    }

    /// The side a line anchor reads, and the default off a line.
    #[must_use]
    pub fn side(&self) -> Side {
        match self {
            Anchor::Line { side, .. } => *side,
            _ => Side::default(),
        }
    }

    /// The selection inside the line, if the anchor holds one.
    #[must_use]
    pub fn range(&self) -> Option<CommentRange> {
        match self {
            Anchor::Line { at, .. } => at.range(),
            _ => None,
        }
    }

    /// The text of the line, as the revision the anchor is pinned to
    /// held it.
    #[must_use]
    pub fn line_text(&self) -> Option<&str> {
        match self {
            Anchor::Line { line_text, .. } => line_text.as_deref(),
            _ => None,
        }
    }

    /// Records the text of the line, read from the revision the anchor
    /// is pinned to.
    pub fn snapshot_line_text(&mut self, text: Option<String>) {
        if let Anchor::Line { line_text, .. } = self {
            *line_text = text;
        }
    }
}

/// Why a file, a line and a selection are not an anchor.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnchorError {
    #[error("a line anchor requires a file")]
    LineWithoutFile,
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
/// own note — which is what distinguishes reviewer from author.
#[derive(Debug, Clone, Serialize, Deserialize)]
// Renamed in the generated types: the published rendering already owns
// the `ThreadComment` name.
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
/// shape is `u64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Selection")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommentRange {
    start_line: u64,
    start_char: u64,
    end_line: u64,
    end_char: u64,
}

impl CommentRange {
    /// A selection over the reviewed side, in the coordinates above.
    ///
    /// # Errors
    ///
    /// [`CommentRangeError`], naming the rule the selection broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::CommentRange;
    ///
    /// assert!(CommentRange::new(1, 0, 1, 4).is_ok());
    /// assert!(CommentRange::new(1, 4, 1, 4).is_err());
    /// assert!(CommentRange::new(1, 0, 2, 0).is_err());
    /// ```
    pub fn new(
        start_line: u64,
        start_char: u64,
        end_line: u64,
        end_char: u64,
    ) -> Result<CommentRange, CommentRangeError> {
        if start_line < 1 {
            return Err(CommentRangeError::LineBefore1);
        }
        let forward = start_line < end_line || (start_line == end_line && start_char < end_char);
        if !forward {
            return Err(CommentRangeError::Empty);
        }
        if end_char < 1 {
            return Err(CommentRangeError::EndsBeforeItsAnchor);
        }
        Ok(CommentRange {
            start_line,
            start_char,
            end_line,
            end_char,
        })
    }

    #[must_use]
    pub fn start_line(self) -> u64 {
        self.start_line
    }

    #[must_use]
    pub fn start_char(self) -> u64 {
        self.start_char
    }

    /// The line the range ends on, and the line a ranged thread anchors to.
    #[must_use]
    pub fn end_line(self) -> u64 {
        self.end_line
    }

    #[must_use]
    pub fn end_char(self) -> u64 {
        self.end_char
    }
}

/// Why four coordinates are not a [`CommentRange`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommentRangeError {
    #[error("a range starts on line 1 or later")]
    LineBefore1,
    #[error("a range runs forward and selects at least one character")]
    Empty,
    #[error("a range reaches at least one character into the line it anchors to")]
    EndsBeforeItsAnchor,
}

/// The four coordinates as they cross the wire, before they are a range.
#[derive(Deserialize)]
struct Selection {
    start_line: u64,
    start_char: u64,
    end_line: u64,
    end_char: u64,
}

impl TryFrom<Selection> for CommentRange {
    type Error = CommentRangeError;

    fn try_from(s: Selection) -> Result<CommentRange, CommentRangeError> {
        CommentRange::new(s.start_line, s.start_char, s.end_line, s.end_char)
    }
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
