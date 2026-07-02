//! Comment threads, reviewer drafts, and the selected-text range anchor they
//! share (docs/api.md "Comments").

use serde::{Deserialize, Serialize};

use crate::enums::Side;

/// Selected-text anchor of a line comment: 1-based lines on the comment's
/// side, 0-based chars, `end_char` exclusive, `end_line` = the comment's
/// `line`. The JSON shape is these four fields. They are domain coordinates
/// (always non-negative), so the shape is `u64`; the server's `SQLite`
/// columns are signed, converted at the db boundary like every other id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommentRange {
    pub start_line: u64,
    pub start_char: u64,
    pub end_line: u64,
    pub end_char: u64,
}

/// A published comment thread (docs/api.md "Comment placement").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Thread {
    /// Fold-assigned by creation order (not stored).
    pub id: u64,
    pub change_id: u64,
    /// The revision the thread is pinned to.
    pub revision: u64,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: Side,
    /// Null: whole-line thread.
    pub range: Option<CommentRange>,
    pub line_text: Option<String>,
    pub resolved: bool,
    pub comments: Vec<ThreadComment>,
    pub created_at: String,
    pub updated_at: String,
}

/// One message in a [`Thread`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ThreadComment {
    pub body: String,
    /// The review that published it; null for an agent comment. The client
    /// derives reviewer-vs-agent from this — there is no separate `author`.
    pub review_id: Option<u64>,
    pub created_at: String,
}

/// A reviewer's unpublished comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Draft {
    pub id: u64,
    pub change_id: u64,
    pub thread_id: Option<u64>,
    /// The request's anchor revision; only a new thread uses it.
    pub revision: u64,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: Side,
    pub range: Option<CommentRange>,
    pub line_text: Option<String>,
    /// May be empty for a resolution-only reply draft.
    pub body: String,
    /// The staged thread-resolution decision (false when unset).
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// `POST /api/changes/{id}/drafts` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewDraft {
    pub revision: u64,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub file: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub line: Option<u64>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub side: Option<Side>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub range: Option<CommentRange>,
    pub body: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub thread_id: Option<u64>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub resolved: Option<bool>,
}

/// `PATCH /api/drafts/{id}` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EditDraft {
    pub body: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub resolved: Option<bool>,
}

/// `POST /api/changes/{id}/comments` request — the agent's single
/// comment-posting path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewComment {
    #[serde(default)]
    pub thread_id: Option<u64>,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
    #[serde(default)]
    pub side: Option<Side>,
    #[serde(default)]
    pub range: Option<CommentRange>,
    pub body: String,
    #[serde(default)]
    pub resolved: Option<bool>,
}
