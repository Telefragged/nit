//! The review conversation: threads, where they anchor, and the
//! reviewer's unpublished drafts.

use serde::{Deserialize, Serialize};

use super::ChangeNumber;
use super::ChangeVersion;
use super::CommentInput;
use super::Decision;
use super::RevisionNumber;

/// Which version of a revision a file comment is anchored to.
///
/// `new` is the revision: its commit tree and attachments. `old` is its
/// base: the parent tree and the base attachments. An unspecified side is
/// `new`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, utoipa::ToSchema,
)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Old,
    #[default]
    New,
}

impl Side {
    /// The change version that this side of revision `revision` shows, in
    /// the diff of the revision against its base.
    #[must_use]
    pub fn version(self, revision: RevisionNumber) -> ChangeVersion {
        match self {
            Side::Old => ChangeVersion::Base(revision),
            Side::New => ChangeVersion::Revision(revision),
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

/// Where a comment is anchored: the change, or a place in a file.
///
/// A file anchor sits in one version of the change: a revision, or the
/// base of a revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// The change as a whole.
    Change { revision: RevisionNumber },
    /// A place in a file of one version.
    File {
        version: ChangeVersion,
        path: String,
        anchor: FileAnchor,
    },
}

/// Where inside a file an anchor sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileAnchor {
    /// The whole file.
    File,
    /// One whole line, 1-based.
    Line(u64),
    /// A run of characters.
    Range(CommentRange),
}

impl FileAnchor {
    /// Returns the anchor on `range` when it is set, else on `line`, else
    /// on the whole file.
    #[must_use]
    pub fn from_parts(range: Option<CommentRange>, line: Option<u64>) -> FileAnchor {
        match (range, line) {
            (Some(range), _) => FileAnchor::Range(range),
            (None, Some(line)) => FileAnchor::Line(line),
            (None, None) => FileAnchor::File,
        }
    }

    /// The line that a line or range anchor ends on.
    #[must_use]
    pub fn end_line(self) -> Option<u64> {
        match self {
            FileAnchor::File => None,
            FileAnchor::Line(line) => Some(line),
            FileAnchor::Range(range) => Some(range.end_line()),
        }
    }
}

/// Where a logged comment is anchored within its revision.
///
/// The log records the revision of a comment beside this anchor. A file
/// anchor records the version as a [`Side`] of that revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case", from = "StoredAnchor")]
pub enum LogAnchor {
    /// The change as a whole.
    Change,
    /// A place in a file, on one side of the revision.
    File {
        path: String,
        #[serde(default)]
        side: Side,
        anchor: FileAnchor,
    },
}

/// An anchor in any of its spellings, before it is a [`LogAnchor`].
///
/// The log is append-only, so an entry keeps the spelling it was written
/// with. Parsing resolves every spelling into the same anchor.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredAnchor {
    Change,
    File {
        #[serde(alias = "file")]
        path: String,
        #[serde(default)]
        side: Side,
        #[serde(default)]
        anchor: Option<FileAnchor>,
    },
    Line {
        file: String,
        #[serde(default)]
        side: Side,
        #[serde(default)]
        at: Option<StoredLine>,
        #[serde(default)]
        line: Option<u64>,
        #[serde(default)]
        range: Option<CommentRange>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredLine {
    Whole(u64),
    Selection(CommentRange),
}

impl From<StoredAnchor> for LogAnchor {
    fn from(stored: StoredAnchor) -> LogAnchor {
        match stored {
            StoredAnchor::Change => LogAnchor::Change,
            StoredAnchor::File { path, side, anchor } => LogAnchor::File {
                path,
                side,
                anchor: anchor.unwrap_or(FileAnchor::File),
            },
            StoredAnchor::Line {
                file,
                side,
                at,
                line,
                range,
            } => {
                let (range, line) = match at {
                    Some(StoredLine::Whole(line)) => (None, Some(line)),
                    Some(StoredLine::Selection(range)) => (Some(range), None),
                    // The oldest spelling stored the selection in `range`,
                    // and the line it ends on in `line`.
                    None => (range, line),
                };
                LogAnchor::File {
                    path: file,
                    side,
                    anchor: FileAnchor::from_parts(range, line),
                }
            }
        }
    }
}

impl Anchor {
    /// Returns the anchor of a comment that the log records on `revision`.
    #[must_use]
    pub fn from_log(revision: RevisionNumber, anchor: LogAnchor) -> Anchor {
        match anchor {
            LogAnchor::Change => Anchor::Change { revision },
            LogAnchor::File { path, side, anchor } => Anchor::File {
                version: side.version(revision),
                path,
                anchor,
            },
        }
    }

    /// Returns the revision and the anchor that the log records for this
    /// anchor. The inverse of [`Anchor::from_log`].
    #[must_use]
    pub fn into_log(self) -> (RevisionNumber, LogAnchor) {
        let (revision, side) = (self.revision(), self.side());
        let anchor = match self {
            Anchor::Change { .. } => LogAnchor::Change,
            Anchor::File { path, anchor, .. } => LogAnchor::File { path, side, anchor },
        };
        (revision, anchor)
    }

    /// The revision that the anchor sits on, or that its version is the
    /// base of.
    #[must_use]
    pub fn revision(&self) -> RevisionNumber {
        match self {
            Anchor::Change { revision } => *revision,
            Anchor::File { version, .. } => version.revision(),
        }
    }

    /// The side of its revision that a file anchor is on, and `new` for
    /// a change anchor.
    #[must_use]
    pub fn side(&self) -> Side {
        match self {
            Anchor::File {
                version: ChangeVersion::Base(_),
                ..
            } => Side::Old,
            _ => Side::New,
        }
    }

    /// The path of the file of the anchor, if it has one.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Anchor::Change { .. } => None,
            Anchor::File { path, .. } => Some(path),
        }
    }

    /// The place inside the file of a file anchor.
    #[must_use]
    pub fn file_anchor(&self) -> Option<FileAnchor> {
        match self {
            Anchor::Change { .. } => None,
            Anchor::File { anchor, .. } => Some(*anchor),
        }
    }
}

/// A located, resolvable conversation.
///
/// Its anchor and birth come from its first comment; the `id` is
/// fold-assigned by creation order, never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct ThreadProjection {
    pub id: u64,
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct ThreadComment {
    pub body: String,
    pub review_id: Option<u64>,
    pub created_at: String,
}

/// Selected-text anchor of a range comment.
///
/// 1-based lines on the comment's side, 0-based chars, `end_char`
/// exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
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
    /// [`CommentRangeError`], which identifies the rule the selection broke.
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
#[derive(utoipa::ToSchema)]
pub struct Draft {
    pub id: u64,
    pub change_number: ChangeNumber,
    pub comment: CommentInput,
    pub created_at: String,
    pub updated_at: String,
}

/// A reviewer's draft decision plus its cover note/reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct DraftDecision {
    pub decision: Decision,
    #[serde(default)]
    pub message: String,
}
