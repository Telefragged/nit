// Pure diff-presentation logic, kept out of components so it stays testable.

import type { CommentRange, DiffFile, Hunk, Line } from "../api/types";
import { COMMIT_MSG_PATH } from "../api/types";

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

/** One visual row in side-by-side view. */
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

/**
 * Intraline (word-level) emphasis for a hunk: del runs are aligned with the
 * add run that follows (same pairing as `pairLines`), and each aligned pair
 * gets the common prefix/suffix stripped so only the differing middle is
 * marked. Keyed by line object identity; absent lines render unmarked.
 */
export function intralineMarks(lines: Line[]): Map<Line, IntralineRange> {
  const marks = new Map<Line, IntralineRange>();
  for (const block of diffBlocks(lines)) {
    if ("context" in block) continue;
    const { dels, adds } = block;
    const n = Math.min(dels.length, adds.length);
    for (let k = 0; k < n; k++) {
      const del = dels[k];
      const add = adds[k];
      if (del === undefined || add === undefined) break;
      const pair = intralineDiff(del.text, add.text);
      if (!pair) continue;
      const [delRange, addRange] = pair;
      if (delRange[0] < delRange[1]) marks.set(del, delRange);
      if (addRange[0] < addRange[1]) marks.set(add, addRange);
    }
  }
  return marks;
}

/**
 * Common-prefix/suffix split of an old/new line pair (predictable, no LCS).
 * Returns null when the pair shares too little — emphasis on a mostly
 * rewritten line is noise, so it stays uniformly tinted.
 */
export function intralineDiff(
  oldText: string,
  newText: string,
): [IntralineRange, IntralineRange] | null {
  if (oldText === newText) return null;
  const limit = Math.min(oldText.length, newText.length);
  let prefix = 0;
  while (prefix < limit && oldText[prefix] === newText[prefix]) prefix++;
  let suffix = 0;
  while (
    suffix < limit - prefix &&
    oldText[oldText.length - 1 - suffix] ===
      newText[newText.length - 1 - suffix]
  ) {
    suffix++;
  }
  // Similarity gate: the differing middle of either side must stay well
  // under the full line, or the pair is effectively a rewrite.
  const maxLen = Math.max(oldText.length, newText.length);
  const maxMid = maxLen - prefix - suffix;
  if (maxMid > 0.55 * maxLen) return null;
  return [
    [prefix, oldText.length - suffix],
    [prefix, newText.length - suffix],
  ];
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

/** Lines skipped between the previous hunk (if any) and this one. */
export function skippedBefore(prev: Hunk | undefined, hunk: Hunk): number {
  if (!prev) {
    return Math.max(hunk.old_start - 1, hunk.new_start - 1, 0);
  }
  const oldSkip = hunk.old_start - (prev.old_start + prev.old_lines);
  const newSkip = hunk.new_start - (prev.new_start + prev.new_lines);
  return Math.max(oldSkip, newSkip, 0);
}
