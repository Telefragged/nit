//! Wire types — the Rust mirror of `docs/api.md` (the single source of
//! truth for shapes; the frontend mirror is `web/src/api/types.ts`).
//! Change the doc first, then both mirrors.
//!
//! Enumerated values (statuses, states, verdicts, kinds, sides) are the
//! shared serde enums of [`crate::enums`], re-exported here — never plain
//! `String`s. Their serde renamings reproduce the wire spellings, so the
//! type carries the value end to end (domain → JSON → CLI) and an unknown
//! value is rejected at deserialize time, not deep in a handler.

use serde::{Deserialize, Serialize};

pub use crate::enums::{
    ChainState, ChangeStatus, Decision, FileStatus, GraphSection, LineKind, LogKind, Side, Verdict,
};

// ---------------------------------------------------------------------------
// Health

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub status: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// Errors: non-2xx with {"error": "human readable message"}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

// ---------------------------------------------------------------------------
// Repos
pub use nit_types::repos::{CreateRepo, RelocateRepo, Repo, RepoList};

/// `POST /api/changes/{id}/abandon` request (this is `nit abandon`). The body
/// is optional — an absent or empty `message` abandons without a reason.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AbandonRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Push
pub use nit_types::push::{PushRequest, PushResult, TipChange};

// ---------------------------------------------------------------------------
// Chains (derived; addressed by tip change id + ?revision)
pub use nit_types::chains::{Chain, ChainList, PathEntry};

// ---------------------------------------------------------------------------
// Graph (the spine-centered DAG; docs/api.md "Graph")
pub use nit_types::graph::{GraphNode, RepoGraph};

// ---------------------------------------------------------------------------
// Changes

/// `GET /api/changes/{id}` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub id: u64,
    pub repo_id: u64,
    pub change_key: String,
    /// Ascending.
    pub revisions: Vec<Revision>,
    /// Published threads, all revisions; anchors verbatim (the client places
    /// them by diff range, docs/api.md "Comment placement").
    pub threads: Vec<Thread>,
    /// The reviewer's unpublished comments (drafts), all revisions.
    pub drafts: Vec<Draft>,
    pub reviews: Vec<Review>,
    /// The reviewer's staged decision for this change, or `None`.
    pub draft_decision: Option<StagedDecision>,
}

/// A reviewer's staged decision plus its cover note/reason (docs/api.md
/// "Reviewer decisions"). The body of [`ChangeDetail::draft_decision`] and the
/// `PUT /api/changes/{id}/decision` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedDecision {
    pub decision: Decision,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct Review {
    pub id: u64,
    pub revision: u64,
    pub verdict: Verdict,
    /// Cover message.
    pub message: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Diffs
pub use nit_types::diff::{Diff, DiffFile, Hunk, Line};

// ---------------------------------------------------------------------------
// Comments

/// Selected-text anchor of a line comment (api.md "Range comments").
pub use nit_types::comments::CommentRange;

/// A published comment thread (docs/api.md "Comment placement").
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct ThreadComment {
    pub body: String,
    /// The review that published it; null for an agent comment. The client
    /// derives reviewer-vs-agent from this — there is no separate `author`.
    pub review_id: Option<u64>,
    pub created_at: String,
}

/// A reviewer's unpublished comment (a `drafts`-table row).
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub body: String,
    /// The staged thread-resolution decision (false when unset).
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// `POST /api/changes/{id}/drafts` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDraft {
    pub revision: u64,
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
    pub thread_id: Option<u64>,
    #[serde(default)]
    pub resolved: Option<bool>,
}

/// `PATCH /api/drafts/{id}` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditDraft {
    pub body: String,
    #[serde(default)]
    pub resolved: Option<bool>,
}

// ---------------------------------------------------------------------------
// Reviewer decisions — staged per change (docs/api.md "Reviewer decisions"),
// published per chain (the batch submit below). `StagedDecision` (above) is
// both the stage request and the change-detail field.

/// `POST /api/chains/{id}/submit` response — the outcome of publishing every
/// chain member's staged decision (docs/api.md "Chains").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSubmitResult {
    /// Members whose staged decision published.
    pub submitted: u64,
    /// Members skipped (stale/terminal); their staged decision is kept.
    pub errors: Vec<SubmitError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitError {
    pub change_id: u64,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Agent endpoints

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

/// One log entry (docs/api.md `LogEntry`). Belongs to one change; `seq` totally
/// orders the whole repo, `idx` orders one change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub change_id: u64,
    pub idx: u64,
    pub seq: u64,
    pub kind: LogKind,
    pub created_at: String,
    /// Kind-specific; shapes in data-model.md "Payloads".
    pub payload: serde_json::Value,
}

/// `GET /api/chains/{change_id}/log` response — the aggregated chain log,
/// merged across members and sorted by global `seq`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainLog {
    pub entries: Vec<LogEntry>,
}

// ---------------------------------------------------------------------------
// Events (WS /api/stream) — docs/api.md "Events"

/// A server → client websocket message: a tagged [`LogEntry`], or the
/// out-of-log `new_parent` advisory. `untagged` so an entry serializes as its
/// bare fields and `new_parent` as `{"new_parent": {…}}` — the client tells
/// them apart by the `new_parent` key.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum StreamMsg {
    Entry(LogEntry),
    NewParent { new_parent: NewParent },
}

#[derive(Debug, Clone, Serialize)]
pub struct NewParent {
    /// The child end of the newly established edge (a re-rooted change, or a
    /// brand-new child stacked on `parent`).
    pub of: u64,
    /// The parent `of` now sits on.
    pub parent: u64,
}

/// A client → server websocket message: subscribe a set of changes, each from
/// an idx. Externally tagged. The map keys are **strings** — serde cannot
/// deserialize integer map keys through the content-buffering an
/// internally-tagged/untagged enum needs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMsg {
    Subscribe(std::collections::HashMap<String, u64>),
}
