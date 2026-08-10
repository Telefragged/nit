//! Change detail and the reviewer's draft decision.

use serde::{Deserialize, Serialize};

use crate::comments::{Draft, Thread};
use crate::domain::ChangeId;
use crate::domain::Sha;
use crate::domain::{Decision, Verdict};
use crate::fold::ChangeProjection;

/// The `GET /api/changes` response: matching changes as folded projections.
///
/// The same shape the websocket ships in projection mode. `repo` narrows to
/// one repo (an unknown id matches nothing); `status` is repeatable
/// (`?status={s}&status={s}`) and matches each change's status at its
/// **latest revision** (terminal states win). **No `status` param means
/// every change** — the API bakes in no default subset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeList {
    pub changes: Vec<ChangeProjection>,
}

/// `GET /api/changes/{id}` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeDetail {
    pub id: u64,
    pub repo_id: u64,
    pub change_key: ChangeId,
    /// Ascending.
    pub revisions: Vec<Revision>,
    /// Published threads, all revisions; anchors verbatim.
    ///
    /// The client places them by diff range.
    pub threads: Vec<Thread>,
    /// All revisions.
    pub drafts: Vec<Draft>,
    pub reviews: Vec<Review>,
    pub draft_decision: Option<DraftDecision>,
}

/// `GET /api/changes/{id}/drafts` response.
///
/// The reviewer's private overlay — unpublished drafts and the draft
/// decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeDrafts {
    pub drafts: Vec<Draft>,
    pub draft_decision: Option<DraftDecision>,
}

/// A reviewer's draft decision plus its cover note/reason.
///
/// The body of [`ChangeDetail::draft_decision`] and the
/// `PUT /api/changes/{id}/decision` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DraftDecision {
    pub decision: Decision,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Revision {
    pub number: u64,
    pub commit_sha: Sha,
    pub parent_sha: Sha,
    pub fork_sha: Sha,
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

/// `POST /api/changes/{id}/abandon` request (this is `nit abandon`).
///
/// The body is optional — an absent or empty `message` abandons without
/// a reason.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AbandonRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
