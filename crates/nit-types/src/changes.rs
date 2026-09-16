//! Change detail and the reviewer's draft decision.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::Anchor;
use crate::domain::ChangeId;
use crate::domain::ChangeNumber;
use crate::domain::ChangeProjection;
use crate::domain::ChangeStatus;
use crate::domain::Draft;
use crate::domain::DraftDecision;
use crate::domain::RevisionNumber;
use crate::domain::Sha;
use crate::domain::Tag;
use crate::domain::Tags;
use crate::domain::ThreadProjection;
use crate::domain::Verdict;

/// The query that picks changes: `GET /api/changes` and `POST /api/submit`.
///
/// Every field narrows and an empty field does not, so the empty query
/// picks every change of every repo. `repo` narrows to one repo (an
/// unknown id matches nothing). `status` is repeatable
/// (`?status={s}&status={s}`) and matches each change's status at its
/// **latest revision** (terminal states win). `tag` is repeatable too
/// (`?tag=key=value&tag=key=value`); each one matches the change's tags,
/// verbatim key and value, and every one given must match. There is no
/// prefix, wildcard, or key-only form. `change` picks the change with that
/// number, `change_id` the one with that `Change-Id`. Filters compose, so
/// a tag match admits merged and abandoned changes like any other; narrow
/// with `status` to exclude them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub repo: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<ChangeStatus>,
    /// A malformed pair fails the query deserialization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub tag: Vec<Tag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub change: Option<ChangeNumber>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub change_id: Option<ChangeId>,
}

/// The `GET /api/changes` response: the changes a [`ChangeQuery`] picks,
/// as folded projections.
///
/// The same shape the websocket ships as a `projection` frame.
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

/// One entry of `GET /api/changes/{id}/revisions/{n}/ported`: a thread's
/// anchor carried to a revision it was not written on.
///
/// The thread keeps the anchor it was written with. `anchor` is where that
/// one lands in the trees of `revision`, read off the diff between them:
/// the same lines when the diff left them alone, shifted lines when it
/// inserted or deleted lines above them, and the next place up (the file,
/// then the change) when it rewrote those lines or dropped the file. A
/// ported line anchor carries no `line_text`.
///
/// The response ports every unresolved thread of a revision earlier than
/// `n` to `n`, and with `?against={m}` to `m` as well, `n`'s entries
/// first. `?include_resolved=true` ports the resolved threads too. Entries
/// for one revision are sorted by thread id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PortedComment {
    pub thread_id: u64,
    pub revision: RevisionNumber,
    pub anchor: Anchor,
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
    /// The message's subject ([`subject_of`](crate::domain::subject_of)).
    pub subject: String,
    pub created_at: String,
    /// The change's status at this revision, lifecycle included.
    ///
    /// The last revision's is the change's status.
    pub status: ChangeStatus,
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
