//! Change detail and the reviewer's draft decision.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::ChangeId;
use crate::domain::ChangeNumber;
use crate::domain::ChangeProjection;
use crate::domain::Draft;
use crate::domain::DraftDecision;
use crate::domain::RevisionNumber;
use crate::domain::Sha;
use crate::domain::Tags;
use crate::domain::ThreadProjection;
use crate::domain::Verdict;

/// The `GET /api/changes` response: matching changes as folded projections.
///
/// The same shape the websocket ships in projection mode. `repo` narrows to
/// one repo (an unknown id matches nothing); `status` is repeatable
/// (`?status={s}&status={s}`) and matches each change's status at its
/// **latest revision** (terminal states win). **No `status` param means
/// every change** — the API bakes in no default subset.
///
/// `tag` is repeatable too (`?tag=key=value&tag=key=value`). Each one
/// matches the change's tags, verbatim key and value, and every one
/// given must match. There is no prefix, wildcard,
/// or key-only form. Filters compose, so a tag match admits merged and
/// abandoned changes like any other. Narrow with `status` to exclude
/// them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeList {
    pub changes: Vec<ChangeProjection>,
}

/// `GET /api/changes/{id}` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeDetail {
    pub id: ChangeNumber,
    pub repo_id: u64,
    pub change_id: ChangeId,
    /// Ascending.
    pub revisions: Vec<Revision>,
    /// Every tag the change's `tags` entries have set.
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
    /// Published threads, all revisions; anchors verbatim.
    ///
    /// The client places them by diff range.
    pub threads: Vec<ThreadProjection>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Revision {
    pub number: RevisionNumber,
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
    pub revision: RevisionNumber,
    pub verdict: Verdict,
    /// Cover message.
    pub message: String,
    pub created_at: String,
}

/// `POST /api/changes/{id}/tags` request: the tags to put on a change.
///
/// Labelling is its own action, so it needs no push and no new revision.
/// The tags land as a [`crate::domain::TagsPayload`], which says how they
/// meet the tags the change already carries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TagsRequest {
    pub tags: Tags,
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

/// `GET /api/tags` response: every tag in use across one repo's changes.
///
/// Each change contributes the tags it carries now, so a value a later
/// `tags` entry replaced does not appear. `status` is repeatable
/// (`?status={s}&status={s}`) and admits only the changes at those
/// statuses, as on the change read. Without it, terminal changes
/// contribute too.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TagList {
    /// Each key in use, with its distinct values. Keys and values sorted.
    pub tags: BTreeMap<String, Vec<String>>,
}
