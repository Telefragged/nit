//! The typed payloads of the append-only log (docs/data-model.md "Payloads"):
//! one struct per `kind`, shared by the server's fold and the wire `LogEntry`.

use serde::{Deserialize, Serialize};

use crate::comments::CommentRange;
use crate::enums::{LifecycleAction, Side, Verdict};

/// A `revision` entry: one new commit-sha observed for this change. The
/// revision `number` is **not** carried — the fold mints it (0-based, by
/// append order) so a concurrent shared-change push cannot duplicate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionPayload {
    pub commit_sha: String,
    pub parent_sha: String,
    pub base_sha: String,
    pub message: String,
    /// `false` only for a pure rebase (patch-id-equal, message unchanged): the
    /// new revision then inherits the prior revision's review status rather
    /// than resetting to `pending`.
    pub resets_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPayload {
    pub review_id: u64,
    pub revision: u64,
    pub verdict: Verdict,
    pub message: String,
    /// The drained drafts, in draft order. Each opens a new thread or replies
    /// to an existing one (see [`CommentInput`]).
    pub comments: Vec<CommentInput>,
}

/// A comment inside a `review` or `comment` payload: with `thread_id` unset it
/// **opens a new thread** anchored by the fields below; with it set it
/// **replies** to that thread (the anchor is ignored — the thread owns it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentInput {
    /// `None` opens a new thread; `Some` appends to that thread.
    #[serde(default)]
    pub thread_id: Option<u64>,
    /// Anchor revision for a new thread (a draft's own patchset — an interdiff
    /// old side pins to an earlier revision). The API always stamps it; the
    /// fold falls back to the change's latest only for a malformed payload.
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
    /// New-thread anchor side; `None` on a reply (the thread owns the anchor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    #[serde(default)]
    pub range: Option<CommentRange>,
    #[serde(default)]
    pub line_text: Option<String>,
    pub body: String,
    /// Thread-resolution decision (`Some(true/false)` = resolve/reopen, `None`
    /// = no decision). On a new thread it is the birth state; a `thread_id`
    /// reply with an empty `body` carries only this.
    #[serde(default)]
    pub resolved: Option<bool>,
}

/// A `lifecycle` entry: the merge timer (`merged`) and the `nit abandon` /
/// `nit reopen` actions. `revision` is set only for `merged` (which patchset
/// landed); `message` is an optional reason on `abandoned`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecyclePayload {
    pub action: LifecycleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
