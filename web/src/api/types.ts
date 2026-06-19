// Mirrors docs/api.md exactly — the single source of truth for shapes.
// Never invent shapes in components; change the doc first, then this file
// and crates/nit/src/api/types.rs together.

// ---------------------------------------------------------------------------
// Repos

export interface Repo {
  id: number;
  /** Canonical git-common-dir — the repo's identity and display name. */
  git_dir: string;
  /** The one canonical branch; mergedness tracks it. */
  base_branch: string;
  /** Live tip count (derived from the tip set, never stored). */
  active_chains: number;
}

export interface RepoList {
  repos: Repo[];
}

// ---------------------------------------------------------------------------
// Chains (derived; addressed by tip change id + ?revision)

/** Derived chain state — see the normative state table in docs/api.md.
 * Abandonment is derivation-inert, so there is no abandoned chain state. */
export type ChainState =
  | "merged"
  | "agents_turn"
  | "waiting_for_review"
  | "approved";

/** A change's displayed status at a pinned revision (per (change, revision)). */
export type ChangeStatus =
  | "pending"
  | "approved"
  | "changes_requested"
  | "commented"
  | "merged"
  | "abandoned";

/** One member of a derived path, read at the revision the path pins. */
export interface PathEntry {
  change_id: number;
  /** Position in THIS path (0-based). */
  position: number;
  change_key: string;
  /** The patchset this path walks. */
  revision: number;
  /** The change's newest patchset anywhere; `> revision` drives the client's
   * "newer elsewhere" badge. */
  latest_revision: number;
  /** Per (change, this revision). */
  status: ChangeStatus;
  /** A newer revision of this change landed on the canonical branch. */
  merged_elsewhere: boolean;
  subject: string;
  commit_sha: string;
  short_sha: string;
  /** Scoped to this revision. */
  counts: ChangeCounts;
  /** The reviewer's staged decision for this change, or null. Change-wide
   * (one per change): the same value shows on every chain the change is in —
   * drives the dashboard's draft-state count and batch-submit enable. */
  draft_decision: Decision | null;
}

export interface ChangeCounts {
  /** Published comment threads at this revision. */
  threads: number;
  drafts: number;
  /** Unresolved threads at this revision. */
  unresolved: number;
}

/** A dashboard entry: one per known tip commit. */
export interface ChainSummary {
  tip_change_id: number;
  /** The repo this chain belongs to (registry id). */
  repo_id: number;
  /** Best-effort, resolved at query time. */
  name: string;
  state: ChainState;
  /** The tip's latest revision is partial. */
  partial: boolean;
  /** Newest member-entry time across the path. */
  updated_at: string;
  /** Oldest-first, base → tip. */
  path: PathEntry[];
}

export interface ChainList {
  chains: ChainSummary[];
}

/** The full chain for one tip (the chain page / a change's chain context). */
export interface Chain {
  tip_change_id: number;
  /** The repo this chain belongs to (registry id). */
  repo_id: number;
  name: string;
  base_branch: string;
  state: ChainState;
  partial: boolean;
  path: PathEntry[];
}

/** A tip walking through a change, plus the patchset it pins there. */
export interface ChainRef {
  tip_change_id: number;
  revision: number;
  name: string;
}

// ---------------------------------------------------------------------------
// Changes

export interface ChangeDetail {
  id: number;
  repo_id: number;
  change_key: string;
  subject: string;
  /** Ascending. */
  revisions: Revision[];
  /** Published threads, all revisions; clients filter by the viewing revision. */
  threads: Thread[];
  /** The reviewer's unpublished comments (drafts), all revisions. */
  drafts: Draft[];
  reviews: Review[];
  /** Every tip walking through this change, each with the patchset it pins. */
  chains: ChainRef[];
  /** The reviewer's staged decision for this change, or null. */
  draft_decision: StagedDecision | null;
}

export interface Revision {
  number: number;
  commit_sha: string;
  short_sha: string;
  parent_sha: string;
  base_sha: string;
  partial: boolean;
  /** Full commit message. */
  message: string;
  created_at: string;
}

export type Verdict = "approve" | "request_changes" | "comment";

/** A reviewer's staged decision (docs/api.md "Reviewer decisions"): the review
 * modal's single set of choices — a verdict, or a lifecycle action, so
 * abandonment is a decision rather than a separate button. */
export type Decision = Verdict | "abandon" | "reopen";

/** A staged decision plus its cover note/reason — the body of
 * `ChangeDetail.draft_decision` and the PUT /changes/{id}/decision request. */
export interface StagedDecision {
  decision: Decision;
  message: string;
}

export interface Review {
  id: number;
  revision: number;
  verdict: Verdict;
  /** Cover message. */
  message: string;
  created_at: string;
}

// ---------------------------------------------------------------------------
// Diffs

/**
 * Reserved synthetic diff path: the revision's commit message as a
 * reviewable file, listed first in every diff (docs/api.md "The commit
 * message as a file").
 */
export const COMMIT_MSG_PATH = "/COMMIT_MSG";

export type FileStatus = "added" | "deleted" | "modified" | "renamed";

export interface Diff {
  files: DiffFile[];
}

export interface DiffFile {
  /** New path (old path when deleted). */
  path: string;
  /** Only set for renames. */
  old_path?: string;
  status: FileStatus;
  binary: boolean;
  additions: number;
  deletions: number;
  /** Empty when binary. */
  hunks: Hunk[];
}

export interface Hunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  header: string;
  lines: Line[];
}

export type LineKind = "context" | "add" | "del";

export interface Line {
  kind: LineKind;
  /** Old line number; absent for add. */
  old?: number;
  /** New line number; absent for del. */
  new?: number;
  /** Changed by a rebase, not the agent. Absent (not false) on non-rebased diffs. */
  drift?: boolean;
  /** Without trailing newline. */
  text: string;
}

// ---------------------------------------------------------------------------
// Comments

export type CommentAuthor = "reviewer" | "agent";
export type CommentSide = "old" | "new";

/**
 * Selected-text anchor of a line comment (docs/api.md "Range comments"):
 * 1-based lines on the comment's side, 0-based chars, `end_char`
 * exclusive, `end_line` = the comment's `line`.
 */
export interface CommentRange {
  start_line: number;
  start_char: number;
  end_line: number;
  end_char: number;
}

/** A published comment thread (docs/api.md "Comment placement"). */
export interface Thread {
  /** Fold-assigned by creation order (not stored). */
  id: number;
  change_id: number;
  /** The revision the thread is pinned to. */
  revision: number;
  file: string | null;
  line: number | null;
  /** `new` is `revision`'s commit tree, `old` its parent tree. */
  side: CommentSide;
  /** Null: whole-line thread. */
  range: CommentRange | null;
  /** Snapshot of the anchored line. */
  line_text: string | null;
  resolved: boolean;
  comments: ThreadComment[];
  created_at: string;
  updated_at: string;
}

/** One message in a {@link Thread}. */
export interface ThreadComment {
  author: CommentAuthor;
  body: string;
  /** The review that published it; null for an agent comment. */
  review_id: number | null;
  created_at: string;
}

/** A reviewer's unpublished comment. Opens a new thread (`thread_id` null)
 * or replies to one (`thread_id` set). */
export interface Draft {
  id: number;
  change_id: number;
  thread_id: number | null;
  /** The request's anchor revision; only a new thread uses it. */
  revision: number;
  file: string | null;
  line: number | null;
  side: CommentSide;
  range: CommentRange | null;
  line_text: string | null;
  body: string;
  /** The staged thread-resolution decision (false when unset). */
  resolved: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateDraftRequest {
  revision: number;
  file?: string;
  line?: number;
  side?: CommentSide;
  range?: CommentRange;
  body: string;
  thread_id?: number | null;
  resolved?: boolean;
}

export interface UpdateDraftRequest {
  body: string;
  resolved?: boolean;
}

// ---------------------------------------------------------------------------
// Reviewer decisions — staged per change, published per chain (the batch
// submit). StagedDecision (above) is both the stage request body and the
// ChangeDetail field; the reviewer UI never submits a single review directly.

/** `PUT /api/changes/{id}/decision` request — stage (or overwrite) the
 * change's draft decision. */
export interface StageDecisionRequest {
  decision: Decision;
  message: string;
}

/** `POST /api/chains/{id}/submit` response (docs/api.md "Chains"). */
export interface BatchSubmitResult {
  /** Members whose staged decision published. */
  submitted: number;
  /** Members skipped (stale/terminal); their staged decision is kept. */
  errors: SubmitError[];
}

export interface SubmitError {
  change_id: number;
  message: string;
}

/** `POST /api/changes/{id}/abandon` body (`nit abandon`). The optional
 * `message` records a reason; an absent body abandons without one. */
export interface AbandonRequest {
  message?: string;
}

// ---------------------------------------------------------------------------
// Log (one-shot reads; the live stream returns in a later stage)

/** One log entry (docs/api.md `LogEntry`). */
export interface LogEntry {
  change_id: number;
  /** 0-based position in that change's log. */
  idx: number;
  /** Global, monotone across the repo. */
  seq: number;
  /** revision | review | comment | lifecycle | partial */
  kind: string;
  created_at: string;
  /** Kind-specific; shapes in data-model.md "Payloads". */
  payload: unknown;
}

/** `GET /api/changes/{id}/log` response — one change's slice. */
export interface LogResponse {
  head: number;
  entries: LogEntry[];
}

/** `GET /api/chains/{change_id}/log` response — aggregated, sorted by seq. */
export interface ChainLog {
  entries: LogEntry[];
}

// ---------------------------------------------------------------------------
// Errors

export interface ApiErrorBody {
  error: string;
}
