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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    pub id: i64,
    pub repo_path: String,
    pub branch: String,
    pub base: String,
    /// active | merged | abandoned
    pub status: String,
    /// Derived — api.md state table.
    pub state: String,
    pub last_scan_error: Option<String>,
    pub scan_warnings: Vec<String>,
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
    pub id: i64,
    /// Null while orphaned.
    pub position: Option<i64>,
    pub change_key: String,
    pub subject: String,
    /// pending | approved | changes_requested | commented | orphaned
    pub status: String,
    /// Latest revision number.
    pub revision: i64,
    /// Max revision with a review; null if none.
    pub last_reviewed_revision: Option<i64>,
    pub commit_sha: String,
    pub short_sha: String,
    /// Fixup folding conflicted on the latest revision.
    pub needs_rebase: bool,
    pub counts: ChangeCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeCounts {
    pub revisions: i64,
    pub published_comments: i64,
    pub drafts: i64,
    pub unresolved: i64,
}

// ---------------------------------------------------------------------------
// Changes

/// `GET /api/changes/{id}?revision={n}` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub id: i64,
    pub chain_id: i64,
    pub change_key: String,
    pub position: Option<i64>,
    pub status: String,
    pub subject: String,
    pub last_reviewed_revision: Option<i64>,
    /// Ascending.
    pub revisions: Vec<Revision>,
    /// Published + drafts, all revisions; rendered for the requested
    /// revision (`rendered_line` / `outdated`).
    pub comments: Vec<Comment>,
    pub reviews: Vec<Review>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revision {
    pub number: i64,
    pub commit_sha: String,
    pub short_sha: String,
    pub parent_sha: String,
    /// Full commit message.
    pub message: String,
    pub fixups: Vec<RevisionFixup>,
    pub needs_rebase: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionFixup {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: i64,
    pub revision: i64,
    /// approve | request_changes | comment
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
    pub additions: i64,
    pub deletions: i64,
    /// Empty when binary.
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: i64,
    pub old_lines: i64,
    pub new_start: i64,
    pub new_lines: i64,
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line {
    /// context | add | del
    pub kind: String,
    /// Old line number; absent for add.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<i64>,
    /// New line number; absent for del.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<i64>,
    /// Without trailing newline.
    pub text: String,
}

// ---------------------------------------------------------------------------
// Comments

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: i64,
    pub change_id: i64,
    /// The revision the comment was written against.
    pub revision: i64,
    pub parent_id: Option<i64>,
    /// reviewer | agent
    pub author: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    /// old | new
    pub side: String,
    /// Snapshot of the anchored line.
    pub line_text: Option<String>,
    /// For the requested revision; null with `outdated: true` when the
    /// anchor cannot be ported.
    pub rendered_line: Option<i64>,
    pub outdated: bool,
    pub body: String,
    /// draft | published
    pub state: String,
    pub resolved: bool,
    pub review_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// `POST /api/changes/{id}/drafts` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDraft {
    pub revision: i64,
    /// Optional: change-/file-level comments omit file/line.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<i64>,
    /// Defaults to "new".
    #[serde(default)]
    pub side: Option<String>,
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
}

/// `PATCH /api/drafts/{id}` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditDraft {
    pub body: String,
}

// ---------------------------------------------------------------------------
// Reviews

/// `POST /api/changes/{id}/reviews` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitReview {
    pub revision: i64,
    /// approve | request_changes | comment
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
    /// Marks the thread resolved.
    #[serde(default)]
    pub resolve: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    /// See the api.md state table.
    pub state: String,
    /// ≡ state != waiting_for_review
    pub actionable: bool,
    pub chain: FeedbackChain,
    /// Live changes, chain order.
    pub changes: Vec<FeedbackChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackChain {
    pub id: i64,
    pub branch: String,
    pub base: String,
    pub web_url: String,
    pub last_scan_error: Option<String>,
    pub scan_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackChange {
    pub change_id: i64,
    pub change_key: String,
    pub subject: String,
    pub commit_sha: String,
    /// Latest revision number.
    pub revision: i64,
    pub status: String,
    pub needs_rebase: bool,
    pub unresolved: i64,
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
    pub revision: i64,
}

/// `GET /api/chains/{id}/wait` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitResponse {
    /// Latest event id for this chain; re-poll with it.
    pub cursor: i64,
    pub feedback: Feedback,
}
