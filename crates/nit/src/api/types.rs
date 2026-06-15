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
    Author, ChainState, ChainStatus, ChangeStatus, FileStatus, LineKind, LogKind, Side, Verdict,
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
// Repos (the registry grouping chains; docs/api.md "Repos")

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and display name.
    pub git_dir: String,
    /// Chains not merged/abandoned (computed from the fold, never stored).
    pub active_chains: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoList {
    pub repos: Vec<Repo>,
}

/// `PATCH /api/repos/{id}` request — repoint a moved repo at its new
/// git-common-dir (`nit repo move`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelocateRepo {
    pub git_dir: String,
}

// ---------------------------------------------------------------------------
// Chains

/// `POST /api/chains` request (this is `nit push`). `git_dir` is the repo's
/// canonical git-common-dir (the client infers it; chains group by it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterChain {
    pub git_dir: String,
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
    /// The repo this chain belongs to (registry id) and its git-common-dir.
    pub repo_id: u64,
    pub git_dir: String,
    pub branch: String,
    pub base: String,
    pub status: ChainStatus,
    /// Derived — api.md state table.
    pub state: ChainState,
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
    pub status: ChangeStatus,
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
    /// Published comment threads.
    pub threads: u64,
    pub drafts: u64,
    /// Unresolved threads.
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
    pub status: ChangeStatus,
    pub subject: String,
    pub last_reviewed_revision: Option<u64>,
    /// Ascending.
    pub revisions: Vec<Revision>,
    /// Published threads, all revisions; anchors verbatim (the client places
    /// them by diff range, docs/api.md "Comment placement").
    pub threads: Vec<Thread>,
    /// The reviewer's unpublished comments (drafts), all revisions.
    pub drafts: Vec<Draft>,
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
    pub verdict: Verdict,
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
    pub status: FileStatus,
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
    pub kind: LineKind,
    /// Old line number; absent for add.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<u64>,
    /// New line number; absent for del.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<u64>,
    /// Changed by a rebase, not the agent (docs/api.md "Rebase-aware
    /// interdiffs"). Omitted on the wire when false, so non-rebased diffs
    /// are byte-for-byte unaffected.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub drift: bool,
    /// Without trailing newline.
    pub text: String,
}

// ---------------------------------------------------------------------------
// Comments

/// Selected-text anchor of a line comment (api.md "Range comments") —
/// the db row type is the wire shape verbatim, so it is re-exported
/// rather than mirrored.
pub use crate::db::CommentRange;

/// A published comment thread: its anchor and resolution, plus the
/// conversation on it (docs/api.md "Comment placement").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Fold-assigned by creation order (not stored).
    pub id: u64,
    pub change_id: u64,
    /// The revision the thread is pinned to.
    pub revision: u64,
    pub file: Option<String>,
    pub line: Option<u64>,
    /// `new` is `revision`'s commit tree, `old` its parent tree
    /// (docs/api.md "Comment placement").
    pub side: Side,
    /// Null: whole-line thread.
    pub range: Option<CommentRange>,
    /// Snapshot of the anchored line.
    pub line_text: Option<String>,
    pub resolved: bool,
    pub comments: Vec<ThreadComment>,
    pub created_at: String,
    pub updated_at: String,
}

/// One message in a [`Thread`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadComment {
    pub author: Author,
    pub body: String,
    /// The review that published it; null for an agent comment.
    pub review_id: Option<u64>,
    pub created_at: String,
}

/// A reviewer's unpublished comment (a `drafts`-table row). It opens a new
/// thread (`thread_id` null) or replies to one (`thread_id` set).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: u64,
    pub change_id: u64,
    pub thread_id: Option<u64>,
    /// The request's anchor revision. Meaningful only for a new thread; a
    /// reply ignores it (the thread keeps its own revision).
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
    /// Optional: change-/file-level comments omit file/line.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
    /// Defaults to "new".
    #[serde(default)]
    pub side: Option<Side>,
    /// Optional: requires `line`; api.md "Range comments".
    #[serde(default)]
    pub range: Option<CommentRange>,
    pub body: String,
    /// Set: replies to that thread (on this change). Absent: opens a new
    /// thread anchored by the fields above.
    #[serde(default)]
    pub thread_id: Option<u64>,
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
    pub verdict: Verdict,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitReviewResponse {
    pub review: Review,
    /// The threads this review created or added to.
    pub threads: Vec<Thread>,
}

// ---------------------------------------------------------------------------
// Agent endpoints

/// `POST /api/changes/{id}/comments` request — the agent's single
/// comment-posting path. With `thread_id` set it appends a reply to that
/// thread (on this change); absent, it opens a new thread anchored by the
/// `file`/`line`/`side`/`range` fields like a reviewer draft.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewComment {
    /// Reply: the thread to append to (anchor ignored). Absent: a new thread.
    #[serde(default)]
    pub thread_id: Option<u64>,
    /// New thread only; defaults to the change's latest revision (pass an
    /// earlier one to pin the thread to a prior revision).
    #[serde(default)]
    pub revision: Option<u64>,
    /// Optional: change-level when absent (a `line` requires a `file`).
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
    /// Defaults to "new".
    #[serde(default)]
    pub side: Option<Side>,
    /// Optional but encouraged: requires `line`; api.md "Range comments".
    #[serde(default)]
    pub range: Option<CommentRange>,
    pub body: String,
    /// New thread: initial state (`true` born resolved, else open). Reply:
    /// `true` resolves / `false` reopens / `None` leaves it unchanged.
    #[serde(default)]
    pub resolved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    /// See the api.md state table.
    pub state: ChainState,
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
    pub status: ChangeStatus,
    pub unresolved: u64,
    /// Latest review, null if none.
    pub review: Option<FeedbackReview>,
    /// The latest review's threads, plus still-unresolved threads from
    /// earlier reviews.
    pub threads: Vec<Thread>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackReview {
    pub verdict: Verdict,
    pub message: String,
    pub revision: u64,
}

/// One entry in a chain's log (docs/api.md `LogEntry`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// 0-based position in the chain's log.
    pub idx: u64,
    pub kind: LogKind,
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
