import type { Side } from "../api/types";

/** Lines `start` through `end` on one side of a file's diff. Both ends are
 * inclusive, and `start` never exceeds `end`. */
interface LineSpan {
  side: Side;
  start: number;
  end: number;
}

/** A place in the diff that a link points at: a whole file, or a span of
 * its lines. */
export interface FileLink {
  file: string;
  lines: LineSpan | null;
}

/** A file link that names a span of lines. */
export type LinkSpan = FileLink & { lines: LineSpan };

/** A comment thread that a link points at, by its id. */
interface ThreadLink {
  thread: number;
}

/** A place on the review page that a link points at. */
export type DiffLink = FileLink | ThreadLink;

const LINE = /^(\d+)(?:-(\d+))?$/;

/** Reads the link from the review page's query string.
 *
 * `thread` names a thread by its id. Otherwise `file` names the file,
 * `line` holds `N` or `N-M`, and `side=old` puts the lines on the old side.
 * Returns null without a well-formed `thread` or a `file`. A malformed
 * `line` links the whole file. */
export function parseLink(params: URLSearchParams): DiffLink | null {
  const thread = /^\d+$/.exec(params.get("thread") ?? "");
  if (thread) return { thread: Number(thread[0]) };
  const file = params.get("file");
  if (file === null) return null;
  const m = LINE.exec(params.get("line") ?? "");
  if (!m) return { file, lines: null };
  const a = Number(m[1]);
  const b = Number(m[2] ?? m[1]);
  return {
    file,
    lines: {
      side: params.get("side") === "old" ? "old" : "new",
      start: Math.min(a, b),
      end: Math.max(a, b),
    },
  };
}

/** The query-string patch that writes `link`, the inverse of `parseLink`. */
export function linkParams(link: DiffLink): Record<string, string | null> {
  if ("thread" in link) {
    return { thread: String(link.thread), file: null, line: null, side: null };
  }
  const { lines } = link;
  return {
    thread: null,
    file: link.file,
    line:
      lines === null
        ? null
        : lines.start === lines.end
          ? String(lines.start)
          : `${lines.start}-${lines.end}`,
    side: lines?.side === "old" ? "old" : null,
  };
}

/** Whether two links point at the same place. */
export const sameLink = (a: DiffLink | null, b: DiffLink | null) =>
  a === b ||
  (a !== null &&
    b !== null &&
    JSON.stringify(linkParams(a)) === JSON.stringify(linkParams(b)));

/** The file `link` points into, or null for a thread link. */
export const fileLink = (link: DiffLink | null): FileLink | null =>
  link !== null && "file" in link ? link : null;

/** The thread `link` points at, or null for a file link. */
export const linkedThread = (link: DiffLink | null): number | null =>
  link !== null && "thread" in link ? link.thread : null;

/** Whether `link` points at the thread `id`. A thread without an id is a
 * draft, and no link points at it. */
export const linksThread = (link: DiffLink | null, id: number | null) =>
  id !== null && id === linkedThread(link);

/** Whether `link` covers line `no` on `side` of `file`. */
export const linksLine = (
  link: DiffLink | null,
  file: string,
  side: Side,
  no: number | undefined,
) => {
  const linked = fileLink(link);
  const lines = linked?.file === file ? linked.lines : null;
  return (
    no !== undefined &&
    lines?.side === side &&
    no >= lines.start &&
    no <= lines.end
  );
};

/** The link a click on line `no` makes. With `extend`, the click stretches
 * the current span on the same side of the same file to reach `no`, and
 * keeps the end of the span farther from it. */
export function clickLink(
  current: DiffLink | null,
  file: string,
  side: Side,
  no: number,
  extend: boolean,
): LinkSpan {
  const linked = fileLink(current);
  const cur = linked?.file === file ? linked.lines : null;
  const anchor =
    extend && cur?.side === side ? (no < cur.start ? cur.end : cur.start) : no;
  return {
    file,
    lines: { side, start: Math.min(anchor, no), end: Math.max(anchor, no) },
  };
}
