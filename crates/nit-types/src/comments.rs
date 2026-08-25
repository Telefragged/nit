//! The requests that write a comment.

use serde::{Deserialize, Serialize};

use crate::domain::Anchor;
use crate::domain::RevisionNumber;

/// `POST /api/changes/{id}/drafts` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewDraft {
    pub revision: RevisionNumber,
    /// Where a new thread hangs. A reply keeps the anchor it copies.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub anchor: Option<Anchor>,
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
    pub anchor: Option<Anchor>,
    pub body: String,
    #[serde(default)]
    pub resolved: Option<bool>,
}
