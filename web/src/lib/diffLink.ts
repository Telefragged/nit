import type { Side } from "../api/types";

/** Lines `start` through `end` on one side of a file's diff. Both ends are
 * inclusive, and `start` never exceeds `end`. `chars`, when set, narrows
 * the span to the text from character `chars[0]` of line `start` to
 * character `chars[1]` of line `end`, the end exclusive: one table cell of
 * a rendered Markdown file. */
interface LineSpan {
  side: Side;
  start: number;
  end: number;
  chars?: [number, number];
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

/** A range of characters written as `A-B`, or undefined when `text` is
 * not one. */
export function parseChars(
  text: string | null | undefined,
): [number, number] | undefined {
  const m = /^(\d+)-(\d+)$/.exec(text ?? "");
  return m ? [Number(m[1]), Number(m[2])] : undefined;
}

/** Parses the link from the review page's query string.
 *
 * `thread` is a thread's id. Otherwise `file` is the file's path, `line`
 * is `N` or `N-M`, `chars` is `A-B`, and `side=old` puts the lines on the
 * old side. Returns null without a well-formed `thread` or a
 * `file`. A malformed `line` links the whole file, and a malformed `chars`
 * links the whole lines. */
export function parseLink(params: URLSearchParams): DiffLink | null {
  const thread = /^\d+$/.exec(params.get("thread") ?? "");
  if (thread) return { thread: Number(thread[0]) };
  const file = params.get("file");
  if (file === null) return null;
  const m = LINE.exec(params.get("line") ?? "");
  if (!m) return { file, lines: null };
  const a = Number(m[1]);
  const b = Number(m[2] ?? m[1]);
  const chars = parseChars(params.get("chars"));
  return {
    file,
    lines: {
      side: params.get("side") === "old" ? "old" : "new",
      start: Math.min(a, b),
      end: Math.max(a, b),
      ...(chars && { chars }),
    },
  };
}

/** The query-string patch that writes `link`, the inverse of `parseLink`. */
export function linkParams(link: DiffLink): Record<string, string | null> {
  if ("thread" in link) {
    return {
      thread: String(link.thread),
      file: null,
      line: null,
      chars: null,
      side: null,
    };
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
    chars: lines?.chars ? lines.chars.join("-") : null,
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

/** Whether `link` covers any of lines `no` through `end` on `side` of
 * `file`. `end` defaults to `no`, one line. */
export const linksLine = (
  link: DiffLink | null,
  file: string,
  side: Side,
  no: number | undefined,
  end = no,
) => {
  const linked = fileLink(link);
  const lines = linked?.file === file ? linked.lines : null;
  return (
    no !== undefined &&
    end !== undefined &&
    lines?.side === side &&
    end >= lines.start &&
    no <= lines.end
  );
};

/** Lines `start` through `end` of one side, as a pair. */
export type Lines = [start: number, end: number];

/** The span on `side` of `file` that covers both `a` and `b`. */
export const spanOver = (
  file: string,
  side: Side,
  a: Lines,
  b: Lines,
): LinkSpan => ({
  file,
  lines: {
    side,
    start: Math.min(a[0], b[0]),
    end: Math.max(a[1], b[1]),
  },
});

/** The link a press on `lines` makes, and the lines it keeps while a drag
 * moves its other end. With `extend`, the press stretches the current span
 * on the same side of the same file to reach `lines`. It keeps the span's
 * start, or its end when `lines` start above the span. */
export function pressLink(
  current: DiffLink | null,
  file: string,
  side: Side,
  lines: Lines,
  extend: boolean,
): { span: LinkSpan; fixed: Lines } {
  const linked = fileLink(current);
  const cur = linked?.file === file ? linked.lines : null;
  const fixed: Lines =
    extend && cur?.side === side
      ? lines[0] < cur.start
        ? [cur.end, cur.end]
        : [cur.start, cur.start]
      : lines;
  return { span: spanOver(file, side, fixed, lines), fixed };
}
