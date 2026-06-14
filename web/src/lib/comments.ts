// Comment placement: which column of a diff range a comment renders in,
// and the inverse — which (revision, side) a new draft on a column stores
// to. Pure and side-effect-free (docs/api.md "Comment placement"), so the
// rules are unit-tested without a DOM.

import type { Comment, CommentSide } from "../api/types";

/** A line comment's anchor: the revision and side it is pinned to. */
export interface CommentAnchor {
  revision: number;
  side: CommentSide;
  line: number | null;
}

/** Where a column of the diff cell renders a comment thread. */
export interface Placement {
  side: CommentSide;
  line: number;
}

/**
 * Where a line comment lands in the diff range `[FROM] → [TO]` (`against`
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
  column: CommentSide,
  selected: number,
  against: number | undefined,
): { revision: number; side: CommentSide } {
  if (column === "new") return { revision: selected, side: "new" };
  if (against === undefined) return { revision: selected, side: "old" };
  return { revision: against, side: "new" };
}

/**
 * How many comment threads are anchored to each revision, for the revision
 * dropdowns. Counts roots only (a reply rides with its thread) and both
 * published comments and the reviewer's drafts — the dropdown answers
 * "which revisions carry discussion", and an in-progress draft is
 * discussion too. Keyed by revision number; revisions with none are absent
 * (read with `?? 0`). Not range-filtered: this is each revision's own
 * total, unlike the per-file header count which follows the shown diff.
 */
export function threadCountByRevision(
  comments: readonly Comment[],
): Map<number, number> {
  const counts = new Map<number, number>();
  for (const c of comments) {
    if (c.parent_id !== null) continue;
    counts.set(c.revision, (counts.get(c.revision) ?? 0) + 1);
  }
  return counts;
}

/** "1 comment" / "3 comments" — the count label the revision dropdowns and
 * the file headers share, so the wording stays in one place. */
export function commentCountLabel(n: number): string {
  return `${n} comment${n === 1 ? "" : "s"}`;
}

/**
 * A thread's resolution as it *would* be after the reviewer's pending drafts
 * publish (docs/api.md "Thread resolution"): the newest draft in the thread
 * carries the staged decision, so it wins over the published root's state;
 * with no drafts, the published root's `resolved` stands. `root` and
 * `replies` are one thread (root included so a draft-only thread works).
 */
export function pendingResolved(root: Comment, replies: Comment[]): boolean {
  const staged = [root, ...replies]
    .filter((c) => c.state === "draft")
    .sort((a, b) => a.created_at.localeCompare(b.created_at))
    .at(-1);
  return staged ? staged.resolved : root.resolved;
}

/**
 * How many of a change's threads are unresolved once pending drafts apply —
 * the reviewer-side count shown in the review bar (the server's published
 * count is separate). Counts roots (published or draft) by
 * {@link pendingResolved}.
 */
export function pendingUnresolvedCount(comments: readonly Comment[]): number {
  return comments
    .filter((c) => c.parent_id === null)
    .filter(
      (root) =>
        !pendingResolved(
          root,
          comments.filter((c) => c.parent_id === root.id),
        ),
    ).length;
}
