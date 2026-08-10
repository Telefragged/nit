// @generated from crates/nit-types by `nix run .#gen-types` — DO NOT EDIT.
// Change the Rust wire types, then regenerate.

/**
 * Which tree of a revision a line comment is anchored to.
 *
 * `new` is the revision's commit tree, `old` its parent tree. Defaults
 * to `new` where a request omits it.
 */
export type Side = "old" | "new";

/**
 * A reviewer's verdict on one change.
 *
 * Folds to the matching [`ChangeStatus`] (`From<Verdict>`).
 */
export type Verdict = "approve" | "request_changes" | "comment";

/**
 * A reviewer's **draft** decision on a change.
 *
 * The review modal's single set of choices, drafted in `draft_reviews`
 * and published on batch submit. A superset of [`Verdict`] with the two
 * lifecycle actions, so abandonment is a decision rather than a separate
 * button; it translates back to a [`Verdict`] or a [`LifecycleAction`]
 * at publish time ([`Decision::as_verdict`], [`Decision::as_lifecycle`]).
 */
export type Decision =
  | "approve"
  | "request_changes"
  | "comment"
  | "abandon"
  | "reopen";

/**
 * A change's displayed status at a pinned revision.
 *
 * Per `(change, revision)`, never a change-wide scalar.
 */
export type ChangeStatus =
  | "pending"
  | "approved"
  | "changes_requested"
  | "commented"
  | "merged"
  | "abandoned";

/**
 * A chain's derived, actionable state.
 *
 * Computed at read time from the path's members (the server's
 * `chain::derive_state`); it is informational on the wire, never stored.
 * Abandonment is derivation-inert — there is no abandoned chain state.
 */
export type ChainState =
  | "merged"
  | "authors_turn"
  | "waiting_for_review"
  | "approved";

/**
 * Which region of the change graph a node sits in.
 *
 * `open` ascends above the canonical HEAD, `head` is the HEAD anchor,
 * and `history` descends below it (merged commits, fading with depth).
 * The client styles a node by its `section` first (head → ring,
 * history → grey/fade), falling back to its `ChangeStatus` for open
 * nodes.
 */
export type GraphSection = "open" | "head" | "history";

/**
 * `DiffFile.status` — how a file changed between the two diffed trees.
 */
export type FileStatus = "added" | "deleted" | "modified" | "renamed";

/**
 * `Line.kind` — a diff line's role.
 */
export type LineKind = "context" | "add" | "del";

/**
 * What a `lifecycle` log entry records about a change.
 *
 * The merge/abandon timer writes `merged`/`abandoned`; `nit reopen`
 * writes `reopened`.
 */
export type LifecycleAction = "merged" | "abandoned" | "reopened";

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

/**
 * A derived chain: a tip change's path plus its rolled-up state.
 *
 * The list element (`GET /api/chains`) and the single-chain shape
 * (`GET /api/chains/{id}`) are identical.
 */
export type Chain = {
  tip_change_id: number;
  repo_id: number;
  state: ChainState;
  /**
   * Oldest-first, base → tip.
   */
  path: Array<PathEntry>;
};

/**
 * One member of a derived path: structure only.
 *
 * Read at the revision the path pins. Per-change review state (counts,
 * draft decision, the newest revision) is not here — a client reads it
 * from `GET /api/changes/{id}` per member.
 */
export type PathEntry = {
  change_id: number;
  /**
   * Position in THIS path (0-based).
   */
  position: number;
  change_key: string;
  /**
   * The revision this path walks.
   */
  revision: number;
  /**
   * Per `(change, this revision)`.
   */
  status: ChangeStatus;
  subject: string;
  commit_sha: string;
};

/**
 * One repo's change graph: a commit-sha-keyed DAG over the canonical branch.
 *
 * Not a response body — the browser assembles it (`crates/nit-wasm`) from
 * the two primitive reads, `GET /api/changes` and `GET /api/history`; the
 * shape lives here because it crosses the wasm↔JS boundary.
 */
export type RepoGraph = {
  /**
   * The canonical branch has merged commits below the displayed window — the
   * client shows an "earlier history hidden" marker and dangles deep forks
   * to it.
   */
  history_truncated: boolean;
  /**
   * Row order, top → bottom: open (top) → head → history (bottom).
   *
   * A topological order in which every node precedes its parents.
   */
  nodes: Array<GraphNode>;
};

/**
 * One node of the change graph, keyed by its `commit_sha`.
 *
 * Edges are its `parents` (an edge is drawn to each that is in the node
 * set; `len > 1` is a merge).
 */
export type GraphNode = {
  /**
   * The node's stable id — a full 40-hex commit-sha; the client truncates.
   */
  commit_sha: string;
  section: GraphSection;
  subject: string;
  /**
   * `ChangeStatus` at the pinned revision; head/history read as merged.
   *
   * The client styles by `section`.
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
   * The pinned revision (open nodes); `None` off the open region.
   */
  revision: number | null;
};

/**
 * One commit of the canonical branch's merged history.
 *
 * Walked from the tracked ref's HEAD down (`GET /api/history?repo={id}`).
 */
export type HistoryCommit = {
  /**
   * Full 40-hex commit-sha.
   */
  sha: string;
  /**
   * Parent commit-shas; more than one is a merge.
   */
  parents: Array<string>;
  subject: string;
  /**
   * The merged change this commit carries, matched by its `Change-Id:`
   * trailer. Coupled with `change_key`: a commit whose trailer names no
   * known change (a merge, a pre-nit commit, a foreign trailer) reports
   * both as `None`, never an orphan key.
   */
  change_id: number | null;
  change_key: string | null;
};

/**
 * A window of the canonical branch's merged history (`GET /api/history`).
 *
 * The tracked ref's HEAD first, then its ancestors, a **fixed window of 5
 * commits** deep.
 */
export type RepoHistory = {
  /**
   * HEAD-first; each commit's `parents` carry the edges.
   */
  commits: Array<HistoryCommit>;
  /**
   * The branch has more merged commits below the window.
   */
  truncated: boolean;
};

/**
 * The `GET /api/changes` response: matching changes as folded projections.
 *
 * The same shape the websocket ships in snapshot mode. `repo` narrows to
 * one repo (an unknown id matches nothing); `status` is repeatable
 * (`?status={s}&status={s}`) and matches each change's status at its
 * **latest revision** (terminal states win). **No `status` param means
 * every change** — the API bakes in no default subset.
 */
export type ChangeList = { changes: Array<ChangeProj> };

/**
 * `GET /api/changes/{id}` response.
 */
export type ChangeDetail = {
  id: number;
  repo_id: number;
  change_key: string;
  /**
   * Ascending.
   */
  revisions: Array<Revision>;
  /**
   * Published threads, all revisions; anchors verbatim.
   *
   * The client places them by diff range.
   */
  threads: Array<Thread>;
  /**
   * All revisions.
   */
  drafts: Array<Draft>;
  reviews: Array<Review>;
  draft_decision: DraftDecision | null;
};

/**
 * `GET /api/changes/{id}/drafts` response.
 *
 * The reviewer's private overlay — unpublished drafts and the draft
 * decision.
 */
export type ChangeDrafts = {
  drafts: Array<Draft>;
  draft_decision: DraftDecision | null;
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

/**
 * A reviewer's draft decision plus its cover note/reason.
 *
 * The body of [`ChangeDetail::draft_decision`] and the
 * `PUT /api/changes/{id}/decision` request.
 */
export type DraftDecision = { decision: Decision; message: string };

/**
 * Selected-text anchor of a line comment.
 *
 * 1-based lines on the comment's side, 0-based chars, `end_char`
 * exclusive, `end_line` = the comment's `line`. The JSON shape is these
 * four fields. They are domain coordinates (always non-negative), so the
 * shape is `u64`; the server's `SQLite` columns are signed, converted at
 * the db boundary like every other id.
 */
export type CommentRange = {
  start_line: number;
  start_char: number;
  end_line: number;
  end_char: number;
};

/**
 * A published comment thread.
 */
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

/**
 * One message in a [`Thread`].
 */
export type ThreadComment = {
  body: string;
  /**
   * The review that published it; null for an author comment.
   *
   * The client derives reviewer-vs-author from this — there is no
   * separate `author`.
   */
  review_id: number | null;
  created_at: string;
};

/**
 * A reviewer's unpublished comment.
 */
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
  /**
   * May be empty for a resolution-only reply draft.
   */
  body: string;
  /**
   * The draft's thread-resolution decision (false when unset).
   */
  resolved: boolean;
  created_at: string;
  updated_at: string;
};

/**
 * `POST /api/changes/{id}/drafts` request.
 */
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

/**
 * `PATCH /api/drafts/{id}` request.
 */
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
   * New-side line count; 0 when deleted or binary.
   *
   * The EOF anchor that lets the client reveal the unchanged run below
   * the last hunk, which no hunk bounds from beneath.
   */
  new_total: number;
  /**
   * Empty when binary.
   */
  hunks: Array<Hunk>;
};

/**
 * A file's full-context diff lines.
 *
 * For expanding the unchanged runs the shown diff hides. Same `Line`
 * shape as the diff, so revealed lines carry their drift exactly as the
 * hunks do.
 */
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
   * Changed by a rebase, not by the change itself.
   */
  drift?: boolean;
  /**
   * Without trailing newline.
   */
  text: string;
};

/**
 * `POST /api/chains/{id}/submit` response.
 *
 * The outcome of publishing every chain member's draft decision.
 */
export type BatchSubmitResult = {
  /**
   * Members whose draft decision published.
   */
  submitted: number;
  /**
   * Members skipped (stale/terminal); their draft decision is kept.
   */
  errors: Array<SubmitError>;
};

export type SubmitError = { change_id: number; message: string };

/**
 * A `revision` entry: one new commit-sha observed for this change.
 *
 * The revision `number` is **not** carried — the fold mints it (0-based,
 * by append order) so a concurrent shared-change push cannot duplicate
 * it.
 */
export type RevisionPayload = {
  commit_sha: string;
  parent_sha: string;
  base_sha: string;
  message: string;
  /**
   * `false` only for a pure rebase (patch-id-equal, message unchanged).
   *
   * The new revision then inherits the prior revision's review status
   * rather than resetting to `pending`.
   */
  resets_status: boolean;
};

export type ReviewPayload = {
  revision: number;
  verdict: Verdict;
  message: string;
  /**
   * The drained drafts, in draft order.
   *
   * Each opens a new thread or replies to an existing one (see
   * [`CommentInput`]).
   */
  comments: Array<CommentInput>;
};

/**
 * A comment inside a `review` or `comment` payload.
 *
 * With `thread_id` unset it **opens a new thread** anchored by the
 * fields below; with it set it **replies** to that thread (the anchor is
 * ignored — the thread owns it).
 */
export type CommentInput = {
  /**
   * `None` opens a new thread; `Some` appends to that thread.
   */
  thread_id: number | null;
  /**
   * Anchor revision for a new thread.
   *
   * A draft's own revision — an interdiff old side pins to an earlier
   * revision. The API always stamps it; the fold falls back to the
   * change's latest only for a malformed payload.
   */
  revision: number | null;
  file: string | null;
  line: number | null;
  /**
   * New-thread anchor side; `None` on a reply (the thread owns the anchor).
   */
  side?: Side | null;
  range: CommentRange | null;
  line_text: string | null;
  body: string;
  /**
   * Thread-resolution decision.
   *
   * `Some(true/false)` = resolve/reopen, `None` = no decision. On a new
   * thread it is the birth state; a `thread_id` reply with an empty
   * `body` carries only this.
   */
  resolved: boolean | null;
};

/**
 * A `lifecycle` entry: a merge, an abandon, or a reopen.
 *
 * The merge timer (`merged`) and the `nit abandon` / `nit reopen`
 * actions. `commit_sha` is set only for `merged` — the merged commit on
 * the canonical branch; `message` is an optional reason on `abandoned`.
 */
export type LifecyclePayload = {
  action: LifecycleAction;
  commit_sha?: string | null;
  message?: string | null;
};

/**
 * A log entry's payload as a closed union tagged by `kind`.
 *
 * The server's fold holds it typed; flattened into [`LogEntry`] the
 * adjacent tag produces the wire's `{…, "kind": …, "payload": …}`.
 * Storage serializes the inner struct alone (the `kind` lives in its own
 * column), via the boundary in `crate::review`.
 */
export type LogPayload =
  | { kind: "revision"; payload: RevisionPayload }
  | { kind: "review"; payload: ReviewPayload }
  | { kind: "comment"; payload: CommentInput }
  | { kind: "lifecycle"; payload: LifecyclePayload };

/**
 * One log entry.
 *
 * Belongs to one change; `seq` totally orders the whole repo, `idx`
 * orders one change. The flattened [`LogPayload`] contributes the `kind`
 * discriminant and the `payload` body.
 */
export type LogEntry = {
  change_id: number;
  idx: number;
  seq: number;
  created_at: string;
} & (
  | { kind: "revision"; payload: RevisionPayload }
  | { kind: "review"; payload: ReviewPayload }
  | { kind: "comment"; payload: CommentInput }
  | { kind: "lifecycle"; payload: LifecyclePayload }
);

/**
 * A client → server websocket message. Externally tagged, `snake_case`.
 */
export type ClientMsg =
  | { subscribe: { [key in string]: number } }
  | { subscribe_snapshot: Array<number> };

/**
 * A server → client websocket message. Externally tagged, `snake_case`.
 */
export type StreamMsg = { snapshot: ChangeProj } | { entry: LogEntry };

/**
 * A change's terminal lifecycle, folded from its `lifecycle` entries.
 *
 * The merged commit's sha stays on the `merged` log entry, not here —
 * the fold answers "is it merged", the log answers "as what".
 */
export type Lifecycle = "active" | "merged" | "abandoned";

/**
 * Where a thread is anchored within a revision.
 *
 * Modeled so the invalid combinations the flat wire fields allow are
 * unrepresentable.
 */
export type Anchor =
  | "change"
  | { file: { file: string } }
  | {
      line: {
        file: string;
        side: Side;
        line: number;
        line_text: string | null;
        range: CommentRange | null;
      };
    };

export type RevisionProj = {
  /**
   * 0-based, minted in the fold.
   */
  number: number;
  commit_sha: string;
  parent_sha: string;
  base_sha: string;
  message: string;
  /**
   * `false` for a pure rebase — the revision inherits the prior status.
   */
  resets_status: boolean;
  created_at: string;
};

/**
 * One message in a thread.
 *
 * `review_id` is the review that published it, or `None` for an author's
 * own note — which is what distinguishes reviewer from author (the only
 * consumer derives the label from it).
 */
export type ThreadCommentProj = {
  body: string;
  review_id: number | null;
  created_at: string;
};

/**
 * A located, resolvable conversation.
 *
 * Its anchor and birth come from its first comment; the `id` is
 * fold-assigned by creation order, never stored.
 */
export type ThreadProj = {
  id: number;
  revision: number;
  anchor: Anchor;
  resolved: boolean;
  comments: Array<ThreadCommentProj>;
  created_at: string;
  updated_at: string;
};

export type ReviewProj = {
  /**
   * The `idx` of the `review` entry this is the fold of.
   *
   * A log coordinate, reproduced by replay with nothing stored.
   */
  id: number;
  revision: number;
  verdict: Verdict;
  message: string;
  created_at: string;
};

/**
 * The fold of one change's log.
 *
 * Serializable so the server can ship it as the subscribe **snapshot**
 * and the browser can resume folding the live tail from it; the wire
 * form is opaque to the web, which only passes it back through the
 * shared WebAssembly fold.
 */
export type ChangeProj = {
  id: number;
  repo_id: number;
  change_key: string;
  revisions: Array<RevisionProj>;
  threads: Array<ThreadProj>;
  reviews: Array<ReviewProj>;
  lifecycle: Lifecycle;
  /**
   * Bumped each time a thread is opened.
   */
  next_thread_id: number;
  /**
   * Count of entries folded = the next unconsumed `idx`.
   *
   * A high-water mark, carried in the snapshot so the client resumes
   * folding the live tail at the right boundary and [`fold`] stays
   * idempotent across the overlap.
   */
  entries_folded: number;
};
