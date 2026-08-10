//! The comment endpoints' shapes: a published thread and the requests
//! that write one.

use serde::{Deserialize, Serialize};

use crate::domain::ChangeNumber;
use crate::domain::CommentRange;
use crate::domain::RevisionNumber;
use crate::domain::Side;

/// A published comment thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Thread {
    /// Fold-assigned by creation order (not stored).
    pub id: u64,
    pub change_id: ChangeNumber,
    /// The revision the thread is pinned to.
    pub revision: RevisionNumber,
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
    /// The review that published it; null for an author comment.
    ///
    /// The client derives reviewer-vs-author from this — there is no
    /// separate `author`.
    pub review_id: Option<u64>,
    pub created_at: String,
}

/// `POST /api/changes/{id}/drafts` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewDraft {
    pub revision: RevisionNumber,
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

/// `POST /api/changes/{id}/comments` request.
///
/// The author's single comment-posting path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewComment {
    #[serde(default)]
    pub thread_id: Option<u64>,
    #[serde(default)]
    pub revision: Option<RevisionNumber>,
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
