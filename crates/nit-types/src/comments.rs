//! The requests that write a comment.

use serde::{Deserialize, Serialize};

use crate::domain::Anchor;
use crate::domain::LogAnchor;
use crate::domain::RevisionNumber;

/// `POST /api/changes/{id}/drafts` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NewDraft {
    /// A draft that opens a new thread.
    Thread {
        /// The anchor of the thread.
        anchor: Anchor,
        body: String,
        /// Whether the thread opens resolved.
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        resolved: Option<bool>,
    },
    /// A draft reply to a published thread, on the anchor of that thread.
    Reply {
        thread_id: u64,
        /// May be empty when `resolved` is set.
        body: String,
        /// The resolution decision of the reply.
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional))]
        resolved: Option<bool>,
    },
}

/// `PATCH /api/drafts/{id}` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct EditDraft {
    pub body: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub resolved: Option<bool>,
}

/// `POST /api/changes/{id}/comments` request.
///
/// The author's single comment-posting path.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NewComment {
    #[serde(default)]
    pub thread_id: Option<u64>,
    #[serde(default)]
    pub revision: Option<RevisionNumber>,
    #[serde(default)]
    pub anchor: Option<LogAnchor>,
    pub body: String,
    #[serde(default)]
    pub resolved: Option<bool>,
}
