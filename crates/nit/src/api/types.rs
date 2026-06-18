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
    Author, ChainState, ChangeStatus, FileStatus, LineKind, LogKind, Side, Verdict,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and display name.
    pub git_dir: String,
    /// The one canonical branch; mergedness tracks it.
    pub base_branch: String,
    /// Live tip count (derived from the tip set, never stored).
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
// Push

/// `POST /api/push` request (this is `nit push`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub git_dir: String,
    /// Any ref or rev, resolved to a commit at push time.
    pub tip: String,
    /// The repo's canonical branch (recorded on first push; must match after).
    pub base: String,
    /// Sticky: true marks the tip's revision partial (`nit push --partial`),
    /// false clears it (`nit ready`), absent leaves it unchanged.
    #[serde(default)]
    pub partial: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResult {
    /// The pushed tip change, at the revision this push gave it. `None` when
    /// the push walked to nothing (tip ancestor-or-equal of base).
    pub tip_change: Option<TipChange>,
    /// The derived path, tip-rooted (each member at this push's revision).
    pub chain: Chain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TipChange {
    pub change_id: u64,
    pub change_key: String,
    pub revision: u64,
    pub status: ChangeStatus,
}

// ---------------------------------------------------------------------------
// Chains (derived; addressed by tip change id + ?revision)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSummary {
    pub tip_change_id: u64,
    /// The repo this chain belongs to (registry id).
    pub repo_id: u64,
    /// Best-effort, resolved at query time.
    pub name: String,
    pub state: ChainState,
    /// The tip's latest revision is partial.
    pub partial: bool,
    pub web_url: String,
    /// Newest member-entry time across the path.
    pub updated_at: String,
    /// Oldest-first, base → tip.
    pub path: Vec<PathEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainList {
    pub chains: Vec<ChainSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    pub tip_change_id: u64,
    /// The repo this chain belongs to (registry id).
    pub repo_id: u64,
    pub name: String,
    pub base_branch: String,
    pub state: ChainState,
    pub partial: bool,
    pub web_url: String,
    pub path: Vec<PathEntry>,
}

/// One member of a derived path, read at the revision the path pins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathEntry {
    pub change_id: u64,
    /// Position in THIS path (0-based).
    pub position: u64,
    pub change_key: String,
    /// The patchset this path walks.
    pub revision: u64,
    /// The change's newest patchset anywhere.
    pub latest_revision: u64,
    /// `latest_revision > revision` (badge driver).
    pub newer_elsewhere: bool,
    /// Per `(change, this revision)`.
    pub status: ChangeStatus,
    /// A newer revision of this change landed on the canonical branch.
    pub merged_elsewhere: bool,
    pub subject: String,
    pub commit_sha: String,
    pub short_sha: String,
    /// Scoped to this revision.
    pub counts: ChangeCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeCounts {
    /// Published comment threads at this revision.
    pub threads: u64,
    pub drafts: u64,
    /// Unresolved threads at this revision.
    pub unresolved: u64,
}

/// A tip walking through a change, plus the patchset it pins there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRef {
    pub tip_change_id: u64,
    pub revision: u64,
    pub name: String,
    pub web_url: String,
}

// ---------------------------------------------------------------------------
// Changes

/// `GET /api/changes/{id}` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub id: u64,
    pub repo_id: u64,
    pub change_key: String,
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
    /// Every tip walking through this change, each with the patchset it pins.
    pub chains: Vec<ChainRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revision {
    pub number: u64,
    pub commit_sha: String,
    pub short_sha: String,
    pub parent_sha: String,
    pub base_sha: String,
    pub partial: bool,
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
    /// interdiffs"). Omitted on the wire when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub drift: bool,
    /// Without trailing newline.
    pub text: String,
}

// ---------------------------------------------------------------------------
// Comments

/// Selected-text anchor of a line comment (api.md "Range comments") —
/// the db row type is the wire shape verbatim, re-exported not mirrored.
pub use crate::db::CommentRange;

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
    pub author: Author,
    pub body: String,
    /// The review that published it; null for an agent comment.
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

/// `GET /api/changes/{id}/log` response — one change's slice. `head` is the
/// change's per-change `idx` count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogResponse {
    pub head: u64,
    pub entries: Vec<LogEntry>,
}

/// `GET /api/chains/{change_id}/log` response — the aggregated chain log,
/// merged across members and sorted by global `seq`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainLog {
    pub entries: Vec<LogEntry>,
}
