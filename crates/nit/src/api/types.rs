//! Wire types — the Rust mirror of `docs/api.md` (the single source of
//! truth for shapes; the frontend mirror is `web/src/api/types.ts`).
//! Change the doc first, then both mirrors.
//!
//! Enumerated values (statuses, states, verdicts, kinds, sides) stay
//! plain strings here, exactly as they appear on the wire.

use serde::{Deserialize, Serialize};

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
// Chains

/// `POST /api/chains` request (this is `nit push`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterChain {
    pub repo_path: String,
    pub branch: String,
    pub base: String,
    /// Sticky: true marks the chain partial (`nit push --partial`), false
    /// clears it (`nit ready`), absent leaves it unchanged.
    #[serde(default)]
    pub partial: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    pub id: u64,
    pub repo_path: String,
    pub branch: String,
    pub base: String,
    /// active | merged | abandoned
    pub status: String,
    /// Derived — api.md state table.
    pub state: String,
    /// Sticky; set by push --partial, cleared by ready.
    pub partial: bool,
    pub last_scan_error: Option<String>,
    pub web_url: String,
    pub created_at: String,
    pub updated_at: String,
    /// Chain order; orphaned ones last.
    pub changes: Vec<ChangeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainList {
    pub chains: Vec<Chain>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub id: u64,
    /// Null while orphaned.
    pub position: Option<u64>,
    pub change_key: String,
    pub subject: String,
    /// pending | approved | `changes_requested` | commented | orphaned
    pub status: String,
    /// Latest revision number.
    pub revision: u64,
    /// Max revision with a review; null if none.
    pub last_reviewed_revision: Option<u64>,
    pub commit_sha: String,
    pub short_sha: String,
    pub counts: ChangeCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeCounts {
    pub revisions: u64,
    pub published_comments: u64,
    pub drafts: u64,
    pub unresolved: u64,
}

// ---------------------------------------------------------------------------
// Changes

/// `GET /api/changes/{id}` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub id: u64,
    pub chain_id: u64,
    pub change_key: String,
    pub position: Option<u64>,
    pub status: String,
    pub subject: String,
    pub last_reviewed_revision: Option<u64>,
    /// Ascending.
    pub revisions: Vec<Revision>,
    /// Published + drafts, all revisions; anchors verbatim (the client
    /// places them by diff range, docs/api.md "Comment placement").
    pub comments: Vec<Comment>,
    pub reviews: Vec<Review>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revision {
    pub number: u64,
    pub commit_sha: String,
    pub short_sha: String,
    pub parent_sha: String,
    /// Full commit message.
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: u64,
    pub revision: u64,
    /// approve | `request_changes` | comment
    pub verdict: String,
    /// Cover message.
    pub message: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Diffs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    pub files: Vec<DiffFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFile {
    /// New path (old path when deleted).
    pub path: String,
    /// Only set for renames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// added | deleted | modified | renamed
    pub status: String,
    pub binary: bool,
    pub additions: u64,
    pub deletions: u64,
    /// Empty when binary.
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: u64,
    pub old_lines: u64,
    pub new_start: u64,
    pub new_lines: u64,
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line {
    /// context | add | del
    pub kind: String,
    /// Old line number; absent for add.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<u64>,
    /// New line number; absent for del.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<u64>,
    /// Without trailing newline.
    pub text: String,
}

// ---------------------------------------------------------------------------
// Comments

/// Selected-text anchor of a line comment (api.md "Range comments") —
/// the db row type is the wire shape verbatim, so it is re-exported
/// rather than mirrored.
pub use crate::db::CommentRange;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub change_id: u64,
    /// The revision the comment is pinned to.
    pub revision: u64,
    pub parent_id: Option<u64>,
    /// reviewer | agent
    pub author: String,
    pub file: Option<String>,
    pub line: Option<u64>,
    /// old | new — `new` is `revision`'s commit tree, `old` its parent
    /// tree (docs/api.md "Comment placement").
    pub side: String,
    /// Null: whole-line comment.
    pub range: Option<CommentRange>,
    /// Snapshot of the anchored line.
    pub line_text: Option<String>,
    pub body: String,
    /// draft | published
    pub state: String,
    pub resolved: bool,
    pub review_id: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

/// `POST /api/changes/{id}/drafts` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDraft {
    pub revision: u64,
    /// Optional: change-/file-level comments omit file/line.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
    /// Defaults to "new".
    #[serde(default)]
    pub side: Option<String>,
    /// Optional: requires `line`; api.md "Range comments".
    #[serde(default)]
    pub range: Option<CommentRange>,
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<u64>,
    /// Staged thread-resolution decision (api.md "Thread resolution"); a
    /// reply draft may stage one with an empty body.
    #[serde(default)]
    pub resolved: Option<bool>,
}

/// `PATCH /api/drafts/{id}` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditDraft {
    pub body: String,
    /// Re-stage the resolution decision (api.md "Thread resolution").
    #[serde(default)]
    pub resolved: Option<bool>,
}

// ---------------------------------------------------------------------------
// Reviews

/// `POST /api/changes/{id}/reviews` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitReview {
    pub revision: u64,
    /// approve | `request_changes` | comment
    pub verdict: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitReviewResponse {
    pub review: Review,
    pub published_comments: Vec<Comment>,
}

// ---------------------------------------------------------------------------
// Agent endpoints

/// `POST /api/comments/{id}/replies` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewReply {
    pub body: String,
    /// Thread-resolution decision: `Some(true)` resolves, `Some(false)`
    /// reopens, `None` leaves the thread unchanged.
    #[serde(default)]
    pub resolved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    /// See the api.md state table.
    pub state: String,
    /// ≡ state != `waiting_for_review`
    pub actionable: bool,
    pub chain: FeedbackChain,
    /// Live changes, chain order.
    pub changes: Vec<FeedbackChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackChain {
    pub id: u64,
    pub branch: String,
    pub base: String,
    pub web_url: String,
    /// Sticky; set by push --partial, cleared by ready.
    pub partial: bool,
    pub last_scan_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackChange {
    pub change_id: u64,
    pub change_key: String,
    pub subject: String,
    pub commit_sha: String,
    /// Latest revision number.
    pub revision: u64,
    pub status: String,
    pub unresolved: u64,
    /// Latest review, null if none.
    pub review: Option<FeedbackReview>,
    /// That review's comments only, plus still-unresolved threads from
    /// earlier reviews.
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackReview {
    pub verdict: String,
    pub message: String,
    pub revision: u64,
}

/// One entry in a chain's log (docs/api.md `LogEntry`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// 0-based position in the chain's log.
    pub idx: u64,
    /// revisions | review | reply | partial | `chain_closed`
    pub kind: String,
    pub created_at: String,
    /// Kind-specific; shapes in data-model.md "Payloads".
    pub payload: serde_json::Value,
}

/// `GET /api/chains/{id}/log` response. The `/events` stream emits bare
/// `LogEntry` values (one per SSE event), not a wrapper — the agent-side
/// `head`/feedback view is assembled by the client (`nit wait`) from the
/// stream plus `…/feedback`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogResponse {
    pub head: u64,
    pub entries: Vec<LogEntry>,
}
