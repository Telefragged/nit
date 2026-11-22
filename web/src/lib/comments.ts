// Comment placement: which column of a diff range a thread renders in, and
// the inverse, which version a new draft on a column anchors to. A line
// thread renders at its own anchor only when the version of its anchor is
// a displayed tree. A thread the range hides
// may be shown at the place the server ported it to in a displayed tree
// (`portThreads`); the server stores anchors verbatim and this module is
// the sole owner of the placement rules. Plus assembling the server's
// published threads + reviewer drafts into the UI thread model. Pure and
// side-effect-free, so the rules are unit-tested without a DOM.

import { unresolvedThreads } from "../api/fold";
import { revisionOf, revisionVersion } from "./version";
import type {
  Anchor,
  ChangeDrafts,
  ChangeProjection,
  CommentRange,
  Decision,
  FileAnchor,
  Side,
  Draft,
  PortedComment,
  ThreadProjection,
  ChangeVersion,
  ThreadComment,
} from "../api/types";

export type LineOrRange = Extract<
  FileAnchor,
  { line: unknown } | { range: unknown }
>;

const fileOf = (a: Anchor) => ("file" in a ? a.file : null);

const lineOf = (a: Anchor) => {
  const file = fileOf(a);
  return file && typeof file.anchor === "object"
    ? { version: file.version, at: file.anchor }
    : null;
};

/** Which arm of the anchor union a thread is on. Placement tests this
 * first, and a kind answers it directly: "has no line" is true of a change
 * anchor and a file anchor alike. */
export type AnchorKind = "change" | "file" | "line";

export const anchorKind = (a: Anchor): AnchorKind =>
  "change" in a ? "change" : lineOf(a) ? "line" : "file";

/** The file of an anchor, or null for a change-level one. */
export const anchorFile = (a: Anchor): string | null => fileOf(a)?.path ?? null;

/** The revision that an anchor sits on, or whose base its version is. */
export const anchorRevision = (a: Anchor): number =>
  "change" in a ? a.change.revision : revisionOf(a.file.version);

/** Where in the file an anchor hangs, or null when it has no line. */
export const anchorAt = (a: Anchor): LineOrRange | null =>
  lineOf(a)?.at ?? null;

export const lineVersion = (a: Anchor): ChangeVersion | null =>
  lineOf(a)?.version ?? null;

export const placementLine = (at: LineOrRange): number =>
  "line" in at ? at.line.line : at.range.end_line;

export const firstLine = (at: LineOrRange): number =>
  "line" in at ? at.line.line : at.range.start_line;

export const rangeOf = (at: LineOrRange): CommentRange | null =>
  "range" in at ? at.range : null;

export const anchorRange = (a: Anchor): CommentRange | null => {
  const at = anchorAt(a);
  return at && rangeOf(at);
};

/** The text of the line in the version of the anchor. */
export const anchorLineText = (a: Anchor): string | null => {
  const at = anchorAt(a);
  if (!at) return null;
  return ("line" in at ? at.line.line_text : at.range.line_text) ?? null;
};

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
  anchor: Anchor;
  resolved: boolean;
  /** Published comments (chronological). */
  comments: ThreadComment[];
  /** Pending drafts: reply drafts on this thread, or the lone new-thread
   * draft, oldest first. */
  drafts: Draft[];
  /** When the thread (or its draft) was created — its sort key. */
  created_at: string;
  /** Where a ported comment is shown: a displayed revision's trees, at the
   * place the server ported its anchor to. Absent when the thread is shown
   * at its own anchor. */
  ported?: Anchor;
}

/** The place a thread is shown: its ported place when it has one, else its
 * own anchor. Placement and grouping use this, never `anchor` directly. */
export function shownAt(t: UiThread): Anchor {
  return t.ported ?? t.anchor;
}

/**
 * Marks each thread the range `[FROM] → [TO]` would hide with the first of
 * its `ported` places the range shows. A thread the range already shows
 * keeps its own anchor, so the original is never doubled. The server lists
 * `TO`'s places before `FROM`'s, so the later tree wins: it is where the
 * author's fix would be.
 */
export function portThreads(
  threads: readonly UiThread[],
  ported: readonly PortedComment[],
  selected: number,
  against: ChangeVersion,
): UiThread[] {
  const places = new Map<number, Anchor[]>();
  for (const { thread_id, anchor } of ported) {
    const list = places.get(thread_id) ?? [];
    list.push(anchor);
    places.set(thread_id, list);
  }
  return threads.map((t) => {
    if (t.id === null || threadInRange(t.anchor, selected, against)) return t;
    const at = places
      .get(t.id)
      ?.find((p) => threadInRange(p, selected, against));
    return at ? { ...t, ported: at } : t;
  });
}

/**
 * Merge the server's published threads with the reviewer's drafts into the UI
 * thread model: each published thread collects its reply drafts; each
 * new-thread draft (`thread_id === null`) becomes a draft-only thread. Sorted
 * by creation time, oldest first.
 */
export function assembleThreads(
  threads: readonly ThreadProjection[],
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
      anchor: d.anchor,
      resolved: false,
      comments: [],
      drafts: [d],
      created_at: d.created_at,
    }));
  return [...published, ...draftThreads].sort(byCreated);
}

/**
 * Where a thread lands in the diff range `[against] → [selected]`, or null
 * when the anchor has no line or its version is neither displayed tree.
 * The new column shows `selected`, and the old column shows `against`.
 */
export function commentPlacement(
  anchor: Anchor,
  selected: number,
  against: ChangeVersion,
): Placement | null {
  const place = lineOf(anchor);
  if (place === null) return null;
  const line = placementLine(place.at);
  if (place.version === revisionVersion(selected)) return { side: "new", line };
  if (place.version === against) return { side: "old", line };
  return null;
}

/**
 * The version that a diff column of the range `[against] → [selected]`
 * shows, which a new draft on that column anchors to. The inverse of
 * {@link commentPlacement}.
 */
export const columnVersion = (
  column: Side,
  selected: number,
  against: ChangeVersion,
): ChangeVersion => (column === "new" ? revisionVersion(selected) : against);

/** A diff range as the page shows it: `[against] → [selected]`. */
export interface DiffRange {
  against: ChangeVersion;
  selected: number;
}

/** Whether a thread shows in the range `[FROM] → [TO]`. A line thread needs
 * a column there. A change or file thread has no line, so no single tree
 * contains it and it shows in every range. */
export function threadInRange(
  anchor: Anchor,
  selected: number,
  against: ChangeVersion,
): boolean {
  return (
    anchorKind(anchor) !== "line" ||
    commentPlacement(anchor, selected, against) !== null
  );
}

/**
 * The interdiff from the revision of a thread to the latest revision,
 * `[the thread's revision] → [latest]`, or null when the page must not
 * offer it.
 *
 * There is nothing to offer when the change ends at the thread's own
 * revision, when that range is already on screen, or when the thread has no
 * column in it. The last case is a thread on the base of its revision: an
 * interdiff never shows a base version.
 */
export function rangeSince(
  anchor: Anchor,
  selected: number,
  against: ChangeVersion,
  latest: number,
): DiffRange | null {
  const revision = anchorRevision(anchor);
  if (revision >= latest) return null;
  const range = { against: revisionVersion(revision), selected: latest };
  if (against === range.against && selected === latest) return null;
  return threadInRange(anchor, range.selected, range.against) ? range : null;
}

/**
 * How many threads are anchored to each revision, for the revision pickers.
 * Counts both published and draft-only threads. Keyed by revision number;
 * revisions with none are absent (read with `?? 0`). Not range-filtered:
 * this is each revision's own total.
 */
export function threadCountByRevision(
  threads: readonly UiThread[],
): Map<number, number> {
  const counts = new Map<number, number>();
  for (const t of threads) {
    const revision = anchorRevision(t.anchor);
    counts.set(revision, (counts.get(revision) ?? 0) + 1);
  }
  return counts;
}

/** The count label the revision pickers and the file headers share, so
 * the wording stays in one place. */
export function commentCountLabel(n: number): string {
  return `${n} comment${n === 1 ? "" : "s"}`;
}

/**
 * A thread's resolution as it *would* be after the reviewer's pending drafts
 * publish: the newest draft on the thread carries the draft decision, so it
 * wins over the published state; with no drafts, the published `resolved`
 * stands.
 */
export function pendingResolved(thread: UiThread): boolean {
  const latest = thread.drafts.at(-1); // assembleThreads keeps drafts oldest-first
  return latest ? latest.resolved : thread.resolved;
}

/** The form in which the UI marks a thread: a thread of unsent drafts
 * only, or a published thread that is open or resolved once the pending
 * drafts publish. */
export type ThreadMark = "open" | "draft" | "resolved";

export const threadMark = (t: UiThread): ThreadMark =>
  t.id === null ? "draft" : pendingResolved(t) ? "resolved" : "open";

const MARK_RANK: Record<ThreadMark, number> = {
  resolved: 0,
  draft: 1,
  open: 2,
};

/** The mark that one place shows for two threads: the one of the higher
 * rank, where open is over draft and draft is over resolved. */
export const strongerMark = (
  a: ThreadMark | undefined,
  b: ThreadMark,
): ThreadMark => (a !== undefined && MARK_RANK[a] > MARK_RANK[b] ? a : b);

/** The word for `count` threads with `mark`: the mark, in the plural for
 * several drafts. */
export const markWord = (mark: ThreadMark, count: number): string =>
  mark === "draft" && count !== 1 ? "drafts" : mark;

/** The count of `threads` for each mark that has a thread, in the order
 * draft, open, resolved, with the word for that count. */
export function markCounts(
  threads: readonly UiThread[],
): { mark: ThreadMark; count: number; word: string }[] {
  return (["draft", "open", "resolved"] as const).flatMap((mark) => {
    const count = threads.filter((t) => threadMark(t) === mark).length;
    return count === 0 ? [] : [{ mark, count, word: markWord(mark, count) }];
  });
}

/** `markCounts` as text: "1 draft, 2 open". */
export const markCountsLabel = (threads: readonly UiThread[]) =>
  markCounts(threads)
    .map(({ count, word }) => `${count} ${word}`)
    .join(", ");

/**
 * How many of a change's threads are unresolved once pending drafts apply —
 * the reviewer-side count shown in the review bar (the server's published
 * count is separate).
 */
export function pendingUnresolvedCount(threads: readonly UiThread[]): number {
  return threads.filter((t) => !pendingResolved(t)).length;
}

/** A stable React key for a UiThread: its published id, or the draft-only
 * thread's lone draft id. */
export function threadKey(t: UiThread): string {
  return t.id !== null ? `t${t.id}` : `d${t.drafts[0]?.id ?? ""}`;
}

/** What a node's activity badges draw, derived from the bulk change
 * folds and the reviewer's drafts overlay. */
export interface NodeActivity {
  drafts: readonly Draft[];
  /** Unresolved threads over every revision (`unresolvedThreads`). */
  unresolved: number;
  decision: Decision | null;
}

/** The count of the drafts in `act` that anchor to `revision`. */
export const draftsAt = (
  act: NodeActivity | undefined,
  revision: number | null,
): number =>
  act?.drafts.filter((d) => anchorRevision(d.anchor) === revision).length ?? 0;

/** Each change's activity, keyed by change number: its open threads off
 * its projection, and its drafts and draft decision off the reviewer's
 * overlay (`GET /changes/{id}/drafts`) when that has arrived. */
export function nodeActivity(
  projections: readonly ChangeProjection[],
  overlays: Map<number, ChangeDrafts>,
): Map<number, NodeActivity> {
  return new Map(
    projections.map((p) => {
      const overlay = overlays.get(p.id);
      return [
        p.id,
        {
          drafts: overlay?.drafts ?? [],
          unresolved: unresolvedThreads(p),
          decision: overlay?.draft_decision?.decision ?? null,
        },
      ];
    }),
  );
}
