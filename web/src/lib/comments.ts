// Comment placement: which column of a diff range a thread renders in, and
// the inverse — which (revision, side) a new draft on a column stores to.
// Plus assembling the server's published threads + reviewer drafts into the
// UI thread model. Pure and side-effect-free (docs/api.md "Comment
// placement"), so the rules are unit-tested without a DOM.

import type {
  CommentRange,
  Side,
  Draft,
  Thread,
  ThreadComment,
} from "../api/types";

export interface CommentAnchor {
  revision: number;
  side: Side;
  line: number | null;
}

export interface Placement {
  side: Side;
  line: number;
}

/**
 * A thread as the UI works with it: a published thread merged with the
 * reviewer's pending drafts on it, or a draft-only thread the reviewer has
 * started but not yet published. The anchor and `resolved` come from the
 * published thread; a draft-only thread (`id === null`) takes them from its
 * sole draft and is open until published.
 */
export interface UiThread {
  id: number | null;
  revision: number;
  file: string | null;
  line: number | null;
  side: Side;
  range: CommentRange | null;
  line_text: string | null;
  resolved: boolean;
  /** Published comments (chronological). */
  comments: ThreadComment[];
  /** Pending drafts: reply drafts on this thread, or the lone new-thread
   * draft, oldest first. */
  drafts: Draft[];
  /** When the thread (or its draft) was created — its sort key. */
  created_at: string;
}

/**
 * Merge the server's published threads with the reviewer's drafts into the UI
 * thread model: each published thread collects its reply drafts; each
 * new-thread draft (`thread_id === null`) becomes a draft-only thread. Sorted
 * by creation time, oldest first.
 */
export function assembleThreads(
  threads: readonly Thread[],
  drafts: readonly Draft[],
): UiThread[] {
  const byCreated = (a: { created_at: string }, b: { created_at: string }) =>
    a.created_at.localeCompare(b.created_at);
  const published: UiThread[] = threads.map((t) => ({
    ...t,
    drafts: drafts.filter((d) => d.thread_id === t.id).sort(byCreated),
  }));
  const draftThreads: UiThread[] = drafts
    .filter((d) => d.thread_id === null)
    .map((d) => ({
      id: null,
      revision: d.revision,
      file: d.file,
      line: d.line,
      side: d.side,
      range: d.range,
      line_text: d.line_text,
      resolved: false,
      comments: [],
      drafts: [d],
      created_at: d.created_at,
    }));
  return [...published, ...draftThreads].sort(byCreated);
}

/**
 * Where a thread lands in the diff range `[FROM] → [TO]` (`against`
 * undefined = base, else the interdiff's left revision `rM`), or null when
 * its `(revision, side)` is neither displayed tree — it belongs to another
 * revision and is not shown in this diff at all.
 *
 * - `(TO, new)` → the right/new column;
 * - `(TO, old)` vs base, or `(FROM, new)` in an interdiff → the left/old
 *   column (the old column of an interdiff is the FROM revision's own tree).
 */
export function commentPlacement(
  c: CommentAnchor,
  selected: number,
  against: number | undefined,
): Placement | null {
  if (c.line === null) return null;
  if (c.revision === selected && c.side === "new") {
    return { side: "new", line: c.line };
  }
  if (against === undefined) {
    if (c.revision === selected && c.side === "old") {
      return { side: "old", line: c.line };
    }
  } else if (c.revision === against && c.side === "new") {
    return { side: "old", line: c.line };
  }
  return null;
}

/**
 * The `(revision, side)` a new draft stores to when written on the given
 * diff column of the range `[FROM] → [TO]` — the inverse of
 * {@link commentPlacement}. The old column of an interdiff is the FROM
 * revision's own content, so a draft there anchors to FROM's new side.
 */
export function draftAnchor(
  column: Side,
  selected: number,
  against: number | undefined,
): { revision: number; side: Side } {
  if (column === "new") return { revision: selected, side: "new" };
  if (against === undefined) return { revision: selected, side: "old" };
  return { revision: against, side: "new" };
}

/**
 * How many threads are anchored to each revision, for the revision dropdowns.
 * Counts both published and draft-only threads — the dropdown answers "which
 * revisions carry discussion", and an in-progress draft is discussion too.
 * Keyed by revision number; revisions with none are absent (read with `?? 0`).
 * Not range-filtered: this is each revision's own total, unlike the per-file
 * header count which follows the shown diff.
 */
export function threadCountByRevision(
  threads: readonly UiThread[],
): Map<number, number> {
  const counts = new Map<number, number>();
  for (const t of threads) {
    counts.set(t.revision, (counts.get(t.revision) ?? 0) + 1);
  }
  return counts;
}

/** "1 comment" / "3 comments" — the count label the revision dropdowns and
 * the file headers share, so the wording stays in one place. */
export function commentCountLabel(n: number): string {
  return `${n} comment${n === 1 ? "" : "s"}`;
}

/** A change's published activity at one revision: the comment/draft/unresolved
 * counts an aggregate row (a graph node, a chain-nav member) shows. Recomputed
 * client-side from a change's threads + drafts so those aggregate rows need not
 * denormalize it — the mirror of the server's `change_counts` / `unresolved_at`
 * (docs/api.md), pinned to `revision`. */
export interface RevisionActivity {
  threads: number;
  drafts: number;
  unresolved: number;
}

export function revisionActivity(
  threads: readonly Thread[],
  drafts: readonly Draft[],
  revision: number,
): RevisionActivity {
  const atRevision = threads.filter((t) => t.revision === revision);
  return {
    threads: atRevision.length,
    drafts: drafts.filter((d) => d.revision === revision).length,
    unresolved: atRevision.filter((t) => !t.resolved).length,
  };
}

/**
 * A thread's resolution as it *would* be after the reviewer's pending drafts
 * publish (docs/api.md "Thread resolution"): the newest draft on the thread
 * carries the staged decision, so it wins over the published state; with no
 * drafts, the published `resolved` stands.
 */
export function pendingResolved(thread: UiThread): boolean {
  const staged = thread.drafts.at(-1); // assembleThreads keeps drafts oldest-first
  return staged ? staged.resolved : thread.resolved;
}

/**
 * How many of a change's threads are unresolved once pending drafts apply —
 * the reviewer-side count shown in the review bar (the server's published
 * count is separate).
 */
export function pendingUnresolvedCount(threads: readonly UiThread[]): number {
  return threads.filter((t) => !pendingResolved(t)).length;
}

/** A stable React key for a UiThread: its published id, or the draft-only
 * thread's lone draft id (published threads never share a draft id). */
export function threadKey(t: UiThread): string {
  return t.id !== null ? `t${t.id}` : `d${t.drafts[0]?.id ?? ""}`;
}
