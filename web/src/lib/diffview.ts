// Pure diff-presentation logic, kept out of components so it stays testable.

import { prepareFileTreeInput } from "@pierre/trees";
import type { CommentRange, DiffFile, Hunk, Line } from "../api/types";
import { COMMIT_MSG_PATH } from "../api/types";
import { intraline_marks } from "../wasm/nit_wasm";

/** Display label for a diff path: the synthetic /COMMIT_MSG file reads
 * "Commit message" (gerrit-style); real paths are themselves. */
export function displayPath(path: string): string {
  return path === COMMIT_MSG_PATH ? "Commit message" : path;
}

/** DOM id of a file section, by its index in the diff. The scrollspy and
 * rail navigation use it to find and scroll sections. */
export function fileDomId(index: number): string {
  return `file-${index}`;
}

/** The diff in tree order: the commit message first (it is not a file),
 * then the order the rail's tree itself renders — directories before
 * files, natural digit runs, case-insensitive. Git emits deltas in plain
 * path order, which interleaves directories and files, so the sections
 * would otherwise scroll in an order the rail never reads. */
export function treeOrder(files: DiffFile[]): DiffFile[] {
  const rank = new Map(
    prepareFileTreeInput(
      files.filter((f) => f.path !== COMMIT_MSG_PATH).map((f) => f.path),
    ).paths.map((path, index) => [path, index]),
  );
  return [...files].sort((a, b) => {
    if (a.path === COMMIT_MSG_PATH) return -1;
    if (b.path === COMMIT_MSG_PATH) return 1;
    return (rank.get(a.path) ?? 0) - (rank.get(b.path) ?? 0);
  });
}

const STATUS_LETTER: Record<DiffFile["status"], string> = {
  added: "A",
  deleted: "D",
  modified: "M",
  renamed: "R",
};

/** Status letter for a file's stat box. The commit message is not an
 * added file: it gets none (its empty box keeps flex alignment). */
export function statusLetter(file: DiffFile): string {
  return file.path === COMMIT_MSG_PATH ? "" : STATUS_LETTER[file.status];
}

/** Whole-diff totals for the file-rail title. The synthetic /COMMIT_MSG
 * entry is excluded from the count and the sums alike — it is not a file,
 * and its message churn would distort the code totals. Binary files count
 * as files but contribute 0/0. */
export function diffTotals(files: DiffFile[]): {
  count: number;
  additions: number;
  deletions: number;
} {
  let count = 0;
  let additions = 0;
  let deletions = 0;
  for (const file of files) {
    if (file.path === COMMIT_MSG_PATH) continue;
    count++;
    additions += file.additions;
    deletions += file.deletions;
  }
  return { count, additions, deletions };
}

export interface RowPair {
  left: Line | null;
  right: Line | null;
}

/** A hunk segment: a lone context line, or a replacement run (the del lines
 * followed by the add lines — git emits del before add within a block). */
type DiffBlock = { context: Line } | { dels: Line[]; adds: Line[] };

/** Walk a hunk's lines as blocks, the shared structure behind side-by-side
 * pairing and intraline marking. Each context line yields alone; a run of
 * dels then adds yields as one replacement block. */
function* diffBlocks(lines: Line[]): Generator<DiffBlock> {
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line === undefined) break;
    if (line.kind === "context") {
      yield { context: line };
      i++;
      continue;
    }
    const dels: Line[] = [];
    const adds: Line[] = [];
    while (i < lines.length) {
      const l = lines[i];
      if (l?.kind !== "del") break;
      dels.push(l);
      i++;
    }
    while (i < lines.length) {
      const l = lines[i];
      if (l?.kind !== "add") break;
      adds.push(l);
      i++;
    }
    yield { dels, adds };
  }
}

/**
 * Pair a hunk's lines into side-by-side rows: context lines mirror, del runs
 * align index-wise with the add run that follows (git emits del before add
 * within a replacement block).
 */
export function pairLines(lines: Line[]): RowPair[] {
  const rows: RowPair[] = [];
  for (const block of diffBlocks(lines)) {
    if ("context" in block) {
      rows.push({ left: block.context, right: block.context });
      continue;
    }
    const { dels, adds } = block;
    const n = Math.max(dels.length, adds.length);
    for (let k = 0; k < n; k++) {
      rows.push({ left: dels[k] ?? null, right: adds[k] ?? null });
    }
  }
  return rows;
}

/** Character range [start, end) to emphasize inside a changed line. */
export type IntralineRange = [number, number];

/** What `intraline_marks` returns for one replacement block: the ranges of
 * each of its lines, in the order the block's lines were sent. */
interface RegionMarks {
  old: IntralineRange[][];
  new: IntralineRange[][];
}

/**
 * Intraline emphasis for a hunk, keyed by line object identity; absent lines
 * render unmarked. Each replacement block is marked as a whole (crates/nit-wasm
 * `intraline`) rather than line against line, so a block whose sides differ in
 * length marks throughout and text that moved between lines still pairs up.
 */
export function intralineMarks(lines: Line[]): Map<Line, IntralineRange[]> {
  const blocks = [...diffBlocks(lines)].flatMap((block) =>
    "context" in block || block.dels.length === 0 || block.adds.length === 0
      ? []
      : [block],
  );
  const regions = intraline_marks(
    blocks.map((block) => ({
      old: block.dels.map((line) => line.text),
      new: block.adds.map((line) => line.text),
    })),
  ) as RegionMarks[];

  const marks = new Map<Line, IntralineRange[]>();
  blocks.forEach((block, index) => {
    const region = regions[index];
    if (!region) return;
    const take = (lines: Line[], ranges: IntralineRange[][]) => {
      lines.forEach((line, k) => {
        const on = ranges[k];
        if (on && on.length > 0) marks.set(line, on);
      });
    };
    take(block.dels, region.old);
    take(block.adds, region.new);
  });
  return marks;
}

/**
 * The char window ([start, end) into the line's text) a comment range
 * covers on line `lineNo` of its side, or null when the range misses the
 * line or the window is empty. Offsets clamp to the text (the contract
 * does not validate them against contents — docs/api.md "Range
 * comments"); interior lines are covered whole.
 */
export function rangeSliceOnLine(
  range: CommentRange,
  lineNo: number,
  textLength: number,
): [number, number] | null {
  if (lineNo < range.start_line || lineNo > range.end_line) return null;
  const start = lineNo === range.start_line ? range.start_char : 0;
  const end = lineNo === range.end_line ? range.end_char : textLength;
  const window: [number, number] = [
    Math.min(start, textLength),
    Math.min(end, textLength),
  ];
  return window[0] < window[1] ? window : null;
}

export function skippedBefore(prev: Hunk | undefined, hunk: Hunk): number {
  if (!prev) {
    return Math.max(hunk.old_start - 1, hunk.new_start - 1, 0);
  }
  const oldSkip = hunk.old_start - (prev.old_start + prev.old_lines);
  const newSkip = hunk.new_start - (prev.new_start + prev.new_lines);
  return Math.max(oldSkip, newSkip, 0);
}

/** skippedBefore's counterpart. Unchanged context keeps a hunk's old and new
 * ends in lockstep, so the new side alone bounds the run to `newTotal`. */
export function skippedAfter(last: Hunk | undefined, newTotal: number): number {
  if (!last) return 0;
  return Math.max(newTotal - (last.new_start + last.new_lines - 1), 0);
}

/** The full-context lines that fall in the gap between `prev` and `hunk` —
 * the hidden run a context-expand button reveals (docs/api.md "Expanding
 * context"). `full` is the file's full-context diff; a line belongs to the
 * gap by its new number (`add`/`context`) or old number (`del`), so an
 * all-drift gap's del lines come along. An undefined `hunk` is the run below
 * the last hunk, bounded only by the file's end. Order is preserved. */
export function gapLines(
  full: readonly Line[],
  prev: Hunk | undefined,
  hunk: Hunk | undefined,
): Line[] {
  const oldLo = prev ? prev.old_start + prev.old_lines : 1;
  const newLo = prev ? prev.new_start + prev.new_lines : 1;
  const oldHi = hunk ? hunk.old_start - 1 : Infinity;
  const newHi = hunk ? hunk.new_start - 1 : Infinity;
  return full.filter((l) =>
    l.new !== undefined
      ? l.new >= newLo && l.new <= newHi
      : l.old !== undefined && l.old >= oldLo && l.old <= oldHi,
  );
}
