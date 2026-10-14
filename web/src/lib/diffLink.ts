import type { Side } from "../api/types";

/** Lines `start` through `end` on one side of a file's diff. Both ends are
 * inclusive, and `start` never exceeds `end`. */
interface LineSpan {
  side: Side;
  start: number;
  end: number;
}

/** A place on the review page that a link points at: a whole file, or a
 * span of its lines. */
export interface DiffLink {
  file: string;
  lines: LineSpan | null;
}

const LINE = /^(\d+)(?:-(\d+))?$/;

/** Reads the link from the review page's query string.
 *
 * `file` names the file, `line` holds `N` or `N-M`, and `side=old` puts the
 * lines on the old side. Returns null without a `file`. A malformed `line`
 * links the whole file. */
export function parseLink(params: URLSearchParams): DiffLink | null {
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
  const { lines } = link;
  return {
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
    a.file === b.file &&
    a.lines?.side === b.lines?.side &&
    a.lines?.start === b.lines?.start &&
    a.lines?.end === b.lines?.end);

/** Whether `link` covers line `no` on `side` of `file`. */
export const linksLine = (
  link: DiffLink | null,
  file: string,
  side: Side,
  no: number | undefined,
) =>
  no !== undefined &&
  link?.file === file &&
  link.lines?.side === side &&
  no >= link.lines.start &&
  no <= link.lines.end;

/** The link a click on line `no` makes. With `extend`, the click stretches
 * the current span on the same side of the same file to reach `no`, and
 * keeps the end of the span farther from it. */
export function clickLink(
  current: DiffLink | null,
  file: string,
  side: Side,
  no: number,
  extend: boolean,
): DiffLink {
  const cur = current?.file === file ? current.lines : null;
  const anchor =
    extend && cur?.side === side ? (no < cur.start ? cur.end : cur.start) : no;
  return {
    file,
    lines: { side, start: Math.min(anchor, no), end: Math.max(anchor, no) },
  };
}
