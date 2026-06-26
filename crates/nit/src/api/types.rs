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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and display name.
    pub git_dir: String,
    /// The one canonical base ref; mergedness tracks it.
    pub base_ref: String,
    /// Live tip count (derived from the tip set, never stored).
    pub active_chains: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoList {
    pub repos: Vec<Repo>,
}

/// `POST /api/repos` request — register a repo (`nit repo create`). `base`
/// configures the one canonical base ref; it must resolve to a commit — any
/// git ref, e.g. `origin/main`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepo {
    pub git_dir: String,
    pub base: String,
}

/// `PATCH /api/repos/{id}` request — repoint a moved repo at its new
/// git-common-dir (`nit repo move`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelocateRepo {
    pub git_dir: String,
}

/// `POST /api/changes/{id}/abandon` request (this is `nit abandon`). The body
/// is optional — an absent or empty `message` abandons without a reason.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AbandonRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Push

/// `POST /api/push` request (this is `nit push`). The repo must already be
/// registered (`nit repo create`); the canonical branch is its stored
/// `base_ref`, so push takes no base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub git_dir: String,
    /// Any ref or rev, resolved to a commit at push time.
    pub tip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResult {
    /// The pushed tip change, at the revision this push gave it. Always
    /// present — a push that walks to nothing is rejected (409). Read the
    /// derived chain back with `GET /api/chains/{tip_change.change_id}`.
    pub tip_change: TipChange,
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
pub struct ChainList {
    pub chains: Vec<Chain>,
}

/// A derived chain: the path through a tip change, plus its rolled-up state.
/// The list element (`GET /api/chains`) and the single-chain shape
/// (`GET /api/chains/{id}`) are identical.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    pub tip_change_id: u64,
    /// The repo this chain belongs to (registry id).
    pub repo_id: u64,
    pub state: ChainState,
    /// Oldest-first, base → tip.
    pub path: Vec<PathEntry>,
}

/// One member of a derived path: structure only, read at the revision the path
/// pins. Per-change review state (counts, staged decision, the newest patchset)
/// is not here — a client reads it from `GET /api/changes/{id}` per member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathEntry {
    pub change_id: u64,
    /// Position in THIS path (0-based).
    pub position: u64,
    pub change_key: String,
    /// The patchset this path walks.
    pub revision: u64,
    /// Per `(change, this revision)`.
    pub status: ChangeStatus,
    pub subject: String,
    pub commit_sha: String,
}

// ---------------------------------------------------------------------------
// Graph (the spine-centered DAG; docs/api.md "Graph")

/// One repo's change graph: a single commit-sha-keyed DAG over the canonical
/// branch, the source for the web dashboard. Nothing about it is stored — it
/// is assembled at read time from the same folds + sha index as a chain, plus
/// a git walk of the canonical branch for the merged history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoGraph {
    pub repo_id: u64,
    /// The HEAD node's `commit_sha` — the anchor every region pivots on.
    pub anchor: String,
    /// The canonical branch has merged commits below the displayed window — the
    /// client shows an "earlier history hidden" marker and dangles deep forks
    /// to it.
    pub history_truncated: bool,
    /// Row order, top → bottom: open (top) → head → history (bottom). A
    /// topological order in which every node precedes its parents.
    pub nodes: Vec<GraphNode>,
}

/// One node of the change graph, keyed by its `commit_sha`. Edges are its
/// `parents` (an edge is drawn to each that is in the node set; `len > 1` is
/// a merge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// The node's stable id — a full 40-hex commit-sha; the client truncates.
    pub commit_sha: String,
    pub section: GraphSection,
    pub subject: String,
    /// `ChangeStatus` at the pinned revision; head/history read as merged —
    /// the client styles by `section`.
    pub status: ChangeStatus,
    /// Parent commit-shas; an edge is drawn to each that is in the node set.
    pub parents: Vec<String>,
    /// The backing change, or `None` for a bare git commit (merge / pre-nit).
    pub change_id: Option<u64>,
    pub change_key: Option<String>,
    /// The pinned patchset (open nodes); `None` off the open region.
    pub revision: Option<u64>,
}

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
