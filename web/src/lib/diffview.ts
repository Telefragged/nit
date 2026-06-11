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

/** Lines skipped between the previous hunk (if any) and this one. */
export function skippedBefore(prev: Hunk | undefined, hunk: Hunk): number {
  if (!prev) {
    return Math.max(hunk.old_start - 1, hunk.new_start - 1, 0);
  }
  const oldSkip = hunk.old_start - (prev.old_start + prev.old_lines);
  const newSkip = hunk.new_start - (prev.new_start + prev.new_lines);
  return Math.max(oldSkip, newSkip, 0);
}
