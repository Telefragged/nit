// Mirrors docs/api.md exactly — the single source of truth for shapes.
// Never invent shapes in components; change the doc first, then this file
// and crates/nit/src/api/types.rs together.

// ---------------------------------------------------------------------------
// Health

export interface Health {
  status: "ok";
  version: string;
}

// ---------------------------------------------------------------------------
// Chains

export type ChainStatus = "active" | "merged" | "abandoned";

/** Derived chain state — see the normative state table in docs/api.md. */
export type ChainState =
  | "waiting_for_review"
  | "agents_turn"
  | "approved"
  | "merged"
  | "abandoned";

export type ChangeStatus =
  | "pending"
  | "approved"
  | "changes_requested"
  | "commented"
  | "orphaned";

export interface Chain {
  id: number;
  repo_path: string;
  branch: string;
  base: string;
  status: ChainStatus;
  state: ChainState;
  /** Sticky; set by push --partial, cleared by ready. */
  partial: boolean;
  last_scan_error: string | null;
  web_url: string;
  created_at: string;
  updated_at: string;
  /** Chain order; orphaned ones last. */
  changes: ChangeSummary[];
}

export interface ChangeSummary {
  id: number;
  position: number | null;
  change_key: string;
  subject: string;
  status: ChangeStatus;
  /** Latest revision number. */
  revision: number;
  /** Max revision with a review; null if none. */
  last_reviewed_revision: number | null;
  commit_sha: string;
  short_sha: string;
  counts: ChangeCounts;
}

export interface ChangeCounts {
  revisions: number;
  published_comments: number;
  drafts: number;
  unresolved: number;
}

export interface RegisterChainRequest {
  repo_path: string;
  branch: string;
  base: string;
  /**
   * Sticky: true marks the chain partial (`nit push --partial`), false clears
   * it (`nit ready`), absent leaves it unchanged.
   */
  partial?: boolean;
}

export interface ChainList {
  chains: Chain[];
}

// ---------------------------------------------------------------------------
// Changes

export interface ChangeDetail {
  id: number;
  chain_id: number;
  change_key: string;
  position: number | null;
  status: ChangeStatus;
  subject: string;
  last_reviewed_revision: number | null;
  /** Ascending. */
  revisions: Revision[];
  /** Published + drafts, all revisions. */
  comments: Comment[];
  reviews: Review[];
}

export interface Revision {
  number: number;
  commit_sha: string;
  short_sha: string;
  parent_sha: string;
  /** Full commit message. */
  message: string;
  created_at: string;
}

export type Verdict = "approve" | "request_changes" | "comment";

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
 * message as a file"). Git tree paths cannot start with "/", so it never
 * collides with a real file.
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
  /** Changed by a rebase, not the agent (docs/api.md "Rebase-aware
   * interdiffs"). Absent (not false) on non-rebased diffs. */
  drift?: boolean;
  /** Without trailing newline. */
  text: string;
}

// ---------------------------------------------------------------------------
// Comments

export type CommentAuthor = "reviewer" | "agent";
export type CommentSide = "old" | "new";
export type CommentState = "draft" | "published";

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

export interface Comment {
  id: number;
  change_id: number;
  /** The revision the comment is pinned to. */
  revision: number;
  parent_id: number | null;
  author: CommentAuthor;
  file: string | null;
  line: number | null;
  /** `new` is `revision`'s commit tree, `old` its parent tree
   * (docs/api.md "Comment placement"). */
  side: CommentSide;
  /** Null: whole-line comment. */
  range: CommentRange | null;
  /** Snapshot of the anchored line. */
  line_text: string | null;
  body: string;
  state: CommentState;
  /** Thread resolution (docs/api.md "Thread resolution"): on a published
   * root the thread's current state, on a published reply always false, on
   * a draft the decision staged on its resolve checkbox. */
  resolved: boolean;
  review_id: number | null;
  created_at: string;
  updated_at: string;
}

export interface CreateDraftRequest {
  revision: number;
  /** Optional: change-level comment when absent. */
  file?: string;
  /** Optional: file-level comment when absent. */
  line?: number;
  /** Defaults to "new". */
  side?: CommentSide;
  /** Optional: requires `line`; docs/api.md "Range comments". */
  range?: CommentRange;
  body: string;
  parent_id?: number | null;
  /** Staged thread-resolution decision (docs/api.md "Thread resolution"); a
   * reply draft may stage one with an empty body. */
  resolved?: boolean;
}

export interface UpdateDraftRequest {
  body: string;
  /** Re-stage the resolution decision. */
  resolved?: boolean;
}

// ---------------------------------------------------------------------------
// Reviews

export interface SubmitReviewRequest {
  revision: number;
  verdict: Verdict;
  message: string;
}

export interface SubmitReviewResponse {
  review: Review;
  published_comments: Comment[];
}

// ---------------------------------------------------------------------------
// Agent endpoints

export interface ReplyRequest {
  body: string;
  /** Thread-resolution decision: true resolves, false reopens, omitted
   * leaves it unchanged. */
  resolved?: boolean;
}

export interface Feedback {
  state: ChainState;
  /** ≡ state != waiting_for_review. */
  actionable: boolean;
  chain: FeedbackChain;
  /** Live changes, chain order. */
  changes: FeedbackChange[];
}

export interface FeedbackChain {
  id: number;
  branch: string;
  base: string;
  web_url: string;
  /** Sticky; set by push --partial, cleared by ready. */
  partial: boolean;
  last_scan_error: string | null;
}

export interface FeedbackChange {
  change_id: number;
  change_key: string;
  subject: string;
  commit_sha: string;
  revision: number;
  status: ChangeStatus;
  unresolved: number;
  /** Latest review, null if none. */
  review: FeedbackReview | null;
  /** That review's comments only, plus still-unresolved threads from earlier reviews. */
  comments: Comment[];
}

export interface FeedbackReview {
  verdict: Verdict;
  message: string;
  revision: number;
}

/** One entry in a chain's log (docs/api.md `LogEntry`). */
export interface LogEntry {
  /** 0-based position in the chain's log. */
  idx: number;
  /** revisions | review | reply | partial | chain_closed */
  kind: string;
  created_at: string;
  /** Kind-specific; shapes in data-model.md "Payloads". */
  payload: unknown;
}

/**
 * The agent-side `/events` stream (`GET /api/chains/{id}/events?cursor=`)
 * emits bare `LogEntry` values, one per SSE event — there is no wrapper
 * response. `nit wait` assembles the `head`/feedback view client-side.
 */

/** `GET /api/chains/{id}/log` response. */
export interface LogResponse {
  head: number;
  entries: LogEntry[];
}

// ---------------------------------------------------------------------------
// Errors

export interface ApiErrorBody {
  error: string;
}
