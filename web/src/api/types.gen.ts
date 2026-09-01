// @generated from crates/nit-types by `nix run .#gen-types` — DO NOT EDIT.
// Change the Rust wire types, then regenerate.

/**
 * A change's identity: its `Change-Id` trailer, verbatim.
 *
 * It is carried in the commit message and survives the rewrites review
 * provokes, which is what binds a new revision to the change it revises;
 * a commit sha does not.
 */
export type ChangeId = string;

/**
 * A git object name, in full: 40 hex characters.
 *
 * Only display ever shortens it.
 */
export type Sha = string;

/**
 * Which version of a change: 0-based, in the order the revisions were
 * observed.
 */
export type RevisionNumber = number;

/**
 * A change's number: the handle nit assigns it and everything carries.
 *
 * Scoped to one nit instance, unlike the [`ChangeId`] that travels in the
 * commit message.
 */
export type ChangeNumber = number;

/**
 * Which tree of a revision a line comment is anchored to.
 *
 * `new` is the revision's commit tree, `old` its parent tree. An
 * unspecified side is `new`.
 */
export type Side = "old" | "new";

/**
 * A reviewer's verdict on one change.
 */
export type Verdict = "approve" | "request_changes" | "comment";

/**
 * A reviewer's **draft** decision on a change.
 *
 * Publishing it translates back to a [`Verdict`] or a
 * [`LifecycleAction`] ([`Decision::as_verdict`],
 * [`Decision::as_lifecycle`]).
 */
export type Decision =
  | "approve"
  | "request_changes"
  | "comment"
  | "abandon"
  | "reopen";

/**
 * A change's displayed status at a pinned revision.
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
 * Derived at read time from the path's members, never stored.
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
 * and `history` descends below it: merged commits, oldest deepest.
 */
export type GraphSection = "open" | "head" | "history";

/**
 * How a file changed between the two diffed trees.
 */
export type FileStatus = "added" | "deleted" | "modified" | "renamed";

/**
 * A diff line's role.
 */
export type LineKind = "context" | "add" | "del";

/**
 * How much of a diff is rendered.
 *
 * `Full` renders every line the change touched. `Outline` collapses every
 * function body and drops every import, so that only signatures,
 * doc-comments, types and fields remain — the change read at the altitude
 * of its API surface.
 */
export type DiffMode = "full" | "outline";

/**
 * What a `lifecycle` log entry records about a change.
 */
export type LifecycleAction = "merged" | "abandoned" | "reopened";

/**
 * A tag set: one value per key.
 *
 * The map orders by key, so a set serializes stably and two sets
 * compare verbatim. Only a [`Tag`] enters a set, and that holds for a
 * set arriving over the wire, so every pair in one meets the vocabulary.
 */
export type Tags = { [key in string]: string };

export type Repo = {
  id: number;
  /**
   * Canonical git-common-dir — the repo's identity and display name.
   */
  git_dir: string;
  /**
   * The one canonical ref; mergedness tracks it.
   */
  canonical_ref: string;
  /**
   * Live tip count (derived from the tip set, never stored).
   */
  active_chains: number;
};

export type RepoList = { repos: Array<Repo> };

/**
 * A derived chain: a tip change's path plus its rolled-up state.
 */
export type Chain = {
  tip_change_number: ChangeNumber;
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
 * draft decision, the newest revision) is not here — it belongs to the
 * change itself.
 */
export type PathEntry = {
  change_number: ChangeNumber;
  /**
   * Position in THIS path (0-based).
   */
  position: number;
  change_id: ChangeId;
  /**
   * The revision this path walks.
   */
  revision: RevisionNumber;
  /**
   * Per `(change, this revision)`.
   */
  status: ChangeStatus;
  subject: string;
  commit_sha: Sha;
};

/**
 * One repo's change graph: a commit-sha-keyed DAG over the canonical ref.
 *
 * Not a response body — the browser assembles it (`crates/nit-wasm`) from
 * the two primitive reads, `GET /api/changes` and `GET /api/history`; the
 * shape lives here because it crosses the wasm↔JS boundary.
 */
export type RepoGraph = {
  /**
   * The canonical ref has merged commits below the displayed window — the
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
 * set; `len > 1` is a merge). An open node whose parent is not in the
 * set attaches to its `fork_sha` instead. The commits between the two
 * are not in the graph. When the parent is the fork, the base is older
 * than the displayed window, and nothing is missing.
 */
export type GraphNode = {
  /**
   * The node's stable id.
   */
  commit_sha: Sha;
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
  parents: Array<Sha>;
  /**
   * The backing change, or `None` for a bare git commit (merge / pre-nit).
   */
  change_number: ChangeNumber | null;
  change_id: ChangeId | null;
  /**
   * The pinned revision (open nodes); `None` off the open region.
   */
  revision: RevisionNumber | null;
  /**
   * Where the pinned revision forks from the canonical ref (open
   * nodes); `None` off the open region.
   */
  fork_sha: Sha | null;
};

/**
 * One commit of the canonical ref's merged history.
 *
 * Walked from the tracked ref's HEAD down (`GET /api/history?repo={id}`).
 */
export type HistoryCommit = {
  sha: Sha;
  /**
   * Parent commit-shas; more than one is a merge.
   */
  parents: Array<Sha>;
  subject: string;
  /**
   * The merged change this commit carries, matched by its `Change-Id:`
   * trailer. Coupled with `change_id`: a commit whose trailer names no
   * known change (a merge, a pre-nit commit, a foreign trailer) reports
   * both as `None`, never an orphan key.
   */
  change_number: ChangeNumber | null;
  change_id: ChangeId | null;
};

/**
 * A window of the canonical ref's merged history (`GET /api/history`).
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
 * The same shape the websocket ships in projection mode. `repo` narrows to
 * one repo (an unknown id matches nothing); `status` is repeatable
 * (`?status={s}&status={s}`) and matches each change's status at its
 * **latest revision** (terminal states win). **No `status` param means
 * every change** — the API bakes in no default subset.
 *
 * `tag` is repeatable too (`?tag=key=value&tag=key=value`). Each one
 * matches the change's tags, verbatim key and value, and every one
 * given must match. There is no prefix, wildcard,
 * or key-only form. Filters compose, so a tag match admits merged and
 * abandoned changes like any other. Narrow with `status` to exclude
 * them.
 */
export type ChangeList = { changes: Array<ChangeProjection> };

/**
 * `GET /api/changes/{id}` response.
 */
export type ChangeDetail = {
  id: ChangeNumber;
  repo_id: number;
  change_id: ChangeId;
  /**
   * Ascending.
   */
  revisions: Array<Revision>;
  /**
   * Every tag the change's `tags` entries have set.
   */
  tags?: Tags;
  /**
   * Published threads, all revisions; anchors verbatim.
   *
   * The client places them by diff range.
   */
  threads: Array<ThreadProjection>;
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
  number: RevisionNumber;
  commit_sha: Sha;
  parent_sha: Sha;
  fork_sha: Sha;
  /**
   * Full commit message.
   */
  message: string;
  created_at: string;
};

export type Review = {
  id: number;
  revision: RevisionNumber;
  verdict: Verdict;
  /**
   * Cover message.
   */
  message: string;
  created_at: string;
};

/**
 * A reviewer's draft decision plus its cover note/reason.
 */
export type DraftDecision = { decision: Decision; message: string };

/**
 * Selected-text anchor of a line comment.
 *
 * 1-based lines on the comment's side, 0-based chars, `end_char`
 * exclusive, `end_line` = the comment's `line`. The JSON shape is these
 * four fields. They are domain coordinates (always non-negative), so the
 * shape is `u64`.
 */
export type CommentRange = {
  start_line: number;
  start_char: number;
  end_line: number;
  end_char: number;
};

/**
 * A reviewer's unpublished comment.
 */
export type Draft = {
  id: number;
  change_number: ChangeNumber;
  thread_id: number | null;
  /**
   * The request's anchor revision; only a new thread uses it.
   */
  revision: RevisionNumber;
  anchor: Anchor;
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
  revision: RevisionNumber;
  /**
   * Where a new thread hangs. A reply keeps the anchor it copies.
   */
  anchor?: Anchor;
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
   * The EOF anchor that lets the client reveal the run below
   * the last hunk, which no hunk bounds from beneath.
   */
  new_total: number;
  /**
   * Empty when binary.
   */
  hunks: Array<Hunk>;
};

/**
 * The whole file as diff lines.
 *
 * For expanding the runs the shown diff hides — context beyond a hunk's
 * reach, or a body an outline collapsed. Same `Line`
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

export type SubmitError = { change_number: ChangeNumber; message: string };

/**
 * `GET /api/tags` response: every tag in use across one repo's changes.
 *
 * Each change contributes the tags it carries now, so a value a later
 * `tags` entry replaced does not appear. Terminal changes contribute
 * too. To exclude them, narrow with `?status=` on the change read.
 */
export type TagList = {
  /**
   * Each key in use, with its distinct values. Keys and values sorted.
   */
  tags: { [key in string]: Array<string> };
};

/**
 * A `revision` entry: one new commit-sha observed for this change.
 *
 * The revision `number` is **not** carried — the fold mints it, so a
 * concurrent shared-change push cannot duplicate it.
 */
export type RevisionPayload = {
  commit_sha: Sha;
  parent_sha: Sha;
  fork_sha: Sha;
  message: string;
  /**
   * `false` only for a pure rebase (patch-id-equal, message unchanged).
   *
   * The new revision then inherits the prior revision's review status
   * rather than resetting to `pending`.
   */
  resets_status: boolean;
};

/**
 * A `tags` entry: the labels it puts on a change.
 *
 * The fold lays it over the change's current set, so a key this entry
 * omits keeps the value it had and a key it names takes a new one. The
 * entry stands on its own, so labelling a change costs no revision and
 * disturbs no review status.
 */
export type TagsPayload = { tags: Tags };

export type ReviewPayload = {
  revision: RevisionNumber;
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
 * With `thread_id` unset it **opens a new thread** at its anchor; with
 * it set it **replies** to that thread, which owns the anchor.
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
   * revision. Always set on a recorded comment; the fold falls back to
   * the change's latest only for a malformed payload.
   */
  revision: RevisionNumber | null;
  /**
   * Where a new thread is anchored; `None` on a reply, which takes the
   * anchor its thread already holds.
   */
  anchor: Anchor | null;
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
 * `commit_sha` is set only for `merged` — the merged commit on the
 * canonical ref; `message` is an optional reason on `abandoned`.
 */
export type LifecyclePayload = {
  action: LifecycleAction;
  commit_sha?: Sha | null;
  message?: string | null;
};

/**
 * A log entry's payload as a closed union tagged by `kind`.
 *
 * Flattened into [`LogEntry`], the adjacent tag produces the wire's
 * `{…, "kind": …, "payload": …}`.
 */
export type LogPayload =
  | { kind: "revision"; payload: RevisionPayload }
  | { kind: "review"; payload: ReviewPayload }
  | { kind: "comment"; payload: CommentInput }
  | { kind: "lifecycle"; payload: LifecyclePayload }
  | { kind: "tags"; payload: TagsPayload };

/**
 * One log entry.
 *
 * Belongs to one change; `sequence` totally orders the whole repo, `position`
 * orders one change. The flattened [`LogPayload`] contributes the `kind`
 * discriminant and the `payload` body.
 */
export type LogEntry = {
  change_number: ChangeNumber;
  position: number;
  sequence: number;
  created_at: string;
} & (
  | { kind: "revision"; payload: RevisionPayload }
  | { kind: "review"; payload: ReviewPayload }
  | { kind: "comment"; payload: CommentInput }
  | { kind: "lifecycle"; payload: LifecyclePayload }
  | { kind: "tags"; payload: TagsPayload }
);

/**
 * A client → server websocket message. Externally tagged, `snake_case`.
 */
export type ClientMessage =
  | { subscribe: { [key in string]: number } }
  | { subscribe_projection: Array<ChangeNumber> };

/**
 * A server → client websocket message. Externally tagged, `snake_case`.
 */
export type StreamMessage =
  | { projection: ChangeProjection }
  | { entry: LogEntry };

/**
 * A change's terminal lifecycle, folded from its `lifecycle` entries.
 *
 * The merged commit's sha stays on the `merged` log entry, not here —
 * the fold answers "is it merged", the log answers "as what".
 */
export type Lifecycle = "active" | "merged" | "abandoned";

/**
 * Where a thread is anchored within a revision.
 */
export type Anchor =
  | "change"
  | { file: { file: string } }
  | {
      line: {
        file: string;
        side: Side;
        /**
         * The server snapshots it. A request leaves it unset.
         */
        line_text?: string;
        at: LineAnchor;
      };
    };

/**
 * Where inside a file a line anchor sits.
 *
 * A selection ends on the line it anchors to, so both spellings name
 * exactly one line.
 */
export type LineAnchor = { whole: number } | { selection: CommentRange };

export type RevisionProjection = {
  /**
   * 0-based, minted in the fold.
   */
  number: RevisionNumber;
  commit_sha: Sha;
  parent_sha: Sha;
  fork_sha: Sha;
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
 * own note — which is what distinguishes reviewer from author.
 */
export type ThreadComment = {
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
export type ThreadProjection = {
  id: number;
  revision: RevisionNumber;
  anchor: Anchor;
  resolved: boolean;
  comments: Array<ThreadComment>;
  created_at: string;
  updated_at: string;
};

export type ReviewProjection = {
  /**
   * The `position` of the `review` entry this is the fold of.
   *
   * A log coordinate, reproduced by replay with nothing stored.
   */
  id: number;
  revision: RevisionNumber;
  verdict: Verdict;
  message: string;
  created_at: string;
};

/**
 * The fold of one change's log.
 *
 * Serializable so a fold can be handed on and resumed against the live
 * tail of the log instead of replayed from the start. The wire form is
 * opaque: a projection is only ever produced and consumed by the fold.
 */
export type ChangeProjection = {
  id: ChangeNumber;
  repo_id: number;
  change_id: ChangeId;
  revisions: Array<RevisionProjection>;
  /**
   * What the change's `tags` entries have set so far.
   */
  tags?: Tags;
  threads: Array<ThreadProjection>;
  reviews: Array<ReviewProjection>;
  lifecycle: Lifecycle;
  /**
   * Bumped each time a thread is opened.
   */
  next_thread_id: number;
  /**
   * Count of entries folded = the next unconsumed `position`.
   *
   * A high-water mark, carried in the projection so a resumed fold
   * starts at the right boundary and stays idempotent across the
   * overlap.
   */
  entries_folded: number;
};
