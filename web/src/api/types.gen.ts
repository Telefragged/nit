// @generated from crates/nit-types by `nix run .#gen-types` — DO NOT EDIT.
// Change the Rust wire types (and docs/api.md), then regenerate.

export type Side = "old" | "new";

export type Verdict = "approve" | "request_changes" | "comment";

export type Decision =
  | "approve"
  | "request_changes"
  | "comment"
  | "abandon"
  | "reopen";

export type ChangeStatus =
  | "pending"
  | "approved"
  | "changes_requested"
  | "commented"
  | "merged"
  | "abandoned";

export type ChainState =
  | "merged"
  | "agents_turn"
  | "waiting_for_review"
  | "approved";

export type GraphSection = "open" | "head" | "history";

export type FileStatus = "added" | "deleted" | "modified" | "renamed";

export type LineKind = "context" | "add" | "del";

export type Repo = {
  id: number;
  /**
   * Canonical git-common-dir — the repo's identity and display name.
   */
  git_dir: string;
  /**
   * The one canonical base ref; mergedness tracks it.
   */
  base_ref: string;
  /**
   * Live tip count (derived from the tip set, never stored).
   */
  active_chains: number;
};

export type RepoList = { repos: Array<Repo> };

export type Chain = {
  tip_change_id: number;
  /**
   * The repo this chain belongs to (registry id).
   */
  repo_id: number;
  state: ChainState;
  /**
   * Oldest-first, base → tip.
   */
  path: Array<PathEntry>;
};

export type PathEntry = {
  change_id: number;
  /**
   * Position in THIS path (0-based).
   */
  position: number;
  change_key: string;
  /**
   * The patchset this path walks.
   */
  revision: number;
  /**
   * Per `(change, this revision)`.
   */
  status: ChangeStatus;
  subject: string;
  commit_sha: string;
};

export type RepoGraph = {
  repo_id: number;
  /**
   * The HEAD node's `commit_sha` — the anchor every region pivots on.
   */
  anchor: string;
  /**
   * The canonical branch has merged commits below the displayed window — the
   * client shows an "earlier history hidden" marker and dangles deep forks
   * to it.
   */
  history_truncated: boolean;
  /**
   * Row order, top → bottom: open (top) → head → history (bottom). A
   * topological order in which every node precedes its parents.
   */
  nodes: Array<GraphNode>;
};

export type GraphNode = {
  /**
   * The node's stable id — a full 40-hex commit-sha; the client truncates.
   */
  commit_sha: string;
  section: GraphSection;
  subject: string;
  /**
   * `ChangeStatus` at the pinned revision; head/history read as merged —
   * the client styles by `section`.
   */
  status: ChangeStatus;
  /**
   * Parent commit-shas; an edge is drawn to each that is in the node set.
   */
  parents: Array<string>;
  /**
   * The backing change, or `None` for a bare git commit (merge / pre-nit).
   */
  change_id: number | null;
  change_key: string | null;
  /**
   * The pinned patchset (open nodes); `None` off the open region.
   */
  revision: number | null;
};

export type ChangeDetail = {
  id: number;
  repo_id: number;
  change_key: string;
  /**
   * Ascending.
   */
  revisions: Array<Revision>;
  /**
   * Published threads, all revisions; anchors verbatim (the client places
   * them by diff range, docs/api.md "Comment placement").
   */
  threads: Array<Thread>;
  /**
   * The reviewer's unpublished comments, all revisions.
   */
  drafts: Array<Draft>;
  reviews: Array<Review>;
  /**
   * The reviewer's staged decision.
   */
  draft_decision: StagedDecision | null;
};

export type Revision = {
  number: number;
  commit_sha: string;
  parent_sha: string;
  base_sha: string;
  /**
   * Full commit message.
   */
  message: string;
  created_at: string;
};

export type Review = {
  id: number;
  revision: number;
  verdict: Verdict;
  /**
   * Cover message.
   */
  message: string;
  created_at: string;
};

export type StagedDecision = { decision: Decision; message: string };

export type CommentRange = {
  start_line: number;
  start_char: number;
  end_line: number;
  end_char: number;
};

export type Thread = {
  /**
   * Fold-assigned by creation order (not stored).
   */
  id: number;
  change_id: number;
  /**
   * The revision the thread is pinned to.
   */
  revision: number;
  file: string | null;
  line: number | null;
  side: Side;
  /**
   * Null: whole-line thread.
   */
  range: CommentRange | null;
  line_text: string | null;
  resolved: boolean;
  comments: Array<ThreadComment>;
  created_at: string;
  updated_at: string;
};

export type ThreadComment = {
  body: string;
  /**
   * The review that published it; null for an agent comment. The client
   * derives reviewer-vs-agent from this — there is no separate `author`.
   */
  review_id: number | null;
  created_at: string;
};

export type Draft = {
  id: number;
  change_id: number;
  thread_id: number | null;
  /**
   * The request's anchor revision; only a new thread uses it.
   */
  revision: number;
  file: string | null;
  line: number | null;
  side: Side;
  range: CommentRange | null;
  line_text: string | null;
  body: string;
  /**
   * The staged thread-resolution decision (false when unset).
   */
  resolved: boolean;
  created_at: string;
  updated_at: string;
};

export type NewDraft = {
  revision: number;
  file?: string;
  line?: number;
  side?: Side;
  range?: CommentRange;
  body: string;
  thread_id?: number;
  resolved?: boolean;
};

export type EditDraft = { body: string; resolved?: boolean };

export type Diff = { files: Array<DiffFile> };

export type DiffFile = {
  /**
   * New path (old path when deleted).
   */
  path: string;
  /**
   * Only set for renames.
   */
  old_path?: string;
  status: FileStatus;
  binary: boolean;
  additions: number;
  deletions: number;
  /**
   * Empty when binary.
   */
  hunks: Array<Hunk>;
};

export type FileLines = { lines: Array<Line> };

export type Hunk = {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  header: string;
  lines: Array<Line>;
};

export type Line = {
  kind: LineKind;
  /**
   * Old line number; absent for add.
   */
  old?: number;
  /**
   * New line number; absent for del.
   */
  new?: number;
  /**
   * Changed by a rebase, not the agent (docs/api.md "Rebase-aware
   * interdiffs").
   */
  drift?: boolean;
  /**
   * Without trailing newline.
   */
  text: string;
};

export type BatchSubmitResult = {
  /**
   * Members whose staged decision published.
   */
  submitted: number;
  /**
   * Members skipped (stale/terminal); their staged decision is kept.
   */
  errors: Array<SubmitError>;
};

export type SubmitError = { change_id: number; message: string };
