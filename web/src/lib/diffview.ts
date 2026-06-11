// Pure diff-presentation logic, kept out of components so it stays testable.

import type { Hunk, Line } from "../api/types";

/** One visual row in side-by-side view. */
export interface RowPair {
  left: Line | null;
  right: Line | null;
}

/**
 * Pair a hunk's lines into side-by-side rows: context lines mirror, del runs
 * align index-wise with the add run that follows (git emits del before add
 * within a replacement block).
 */
export function pairLines(lines: Line[]): RowPair[] {
  const rows: RowPair[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (line.kind === "context") {
      rows.push({ left: line, right: line });
      i++;
      continue;
    }
    const dels: Line[] = [];
    const adds: Line[] = [];
    while (i < lines.length && lines[i]!.kind === "del") {
      dels.push(lines[i]!);
      i++;
    }
    while (i < lines.length && lines[i]!.kind === "add") {
      adds.push(lines[i]!);
      i++;
    }
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
  let i = 0;
  while (i < lines.length) {
    if (lines[i]!.kind === "context") {
      i++;
      continue;
    }
    const dels: Line[] = [];
    const adds: Line[] = [];
    while (i < lines.length && lines[i]!.kind === "del") {
      dels.push(lines[i]!);
      i++;
    }
    while (i < lines.length && lines[i]!.kind === "add") {
      adds.push(lines[i]!);
      i++;
    }
    const n = Math.min(dels.length, adds.length);
    for (let k = 0; k < n; k++) {
      const pair = intralineDiff(dels[k]!.text, adds[k]!.text);
      if (!pair) continue;
      const [delRange, addRange] = pair;
      if (delRange[0] < delRange[1]) marks.set(dels[k]!, delRange);
      if (addRange[0] < addRange[1]) marks.set(adds[k]!, addRange);
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
    oldText[oldText.length - 1 - suffix] === newText[newText.length - 1 - suffix]
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

/** Lines skipped between the previous hunk (if any) and this one. */
export function skippedBefore(prev: Hunk | undefined, hunk: Hunk): number {
  if (!prev) {
    return Math.max(hunk.old_start - 1, hunk.new_start - 1, 0);
  }
  const oldSkip = hunk.old_start - (prev.old_start + prev.old_lines);
  const newSkip = hunk.new_start - (prev.new_start + prev.new_lines);
  return Math.max(oldSkip, newSkip, 0);
}
