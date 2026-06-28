//! Change detail and the reviewer's staged decision (docs/api.md "Changes"
//! and "Reviewer decisions").

use serde::{Deserialize, Serialize};

use crate::comments::{Draft, Thread};
use crate::enums::{Decision, Verdict};

/// `GET /api/changes/{id}` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeDetail {
    pub id: u64,
    pub repo_id: u64,
    pub change_key: String,
    /// Ascending.
    pub revisions: Vec<Revision>,
    /// Published threads, all revisions; anchors verbatim (the client places
    /// them by diff range, docs/api.md "Comment placement").
    pub threads: Vec<Thread>,
    /// The reviewer's unpublished comments, all revisions.
    pub drafts: Vec<Draft>,
    pub reviews: Vec<Review>,
    /// The reviewer's staged decision.
    pub draft_decision: Option<StagedDecision>,
}

/// A reviewer's staged decision plus its cover note/reason (docs/api.md
/// "Reviewer decisions"). The body of [`ChangeDetail::draft_decision`] and the
/// `PUT /api/changes/{id}/decision` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct StagedDecision {
    pub decision: Decision,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Revision {
    pub number: u64,
    pub commit_sha: String,
    pub parent_sha: String,
    pub base_sha: String,
    /// Full commit message.
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Review {
    pub id: u64,
    pub revision: u64,
    pub verdict: Verdict,
    /// Cover message.
    pub message: String,
    pub created_at: String,
}

/// `POST /api/changes/{id}/abandon` request (this is `nit abandon`). The body
/// is optional — an absent or empty `message` abandons without a reason.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AbandonRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
