// Collapse state for the review page's file sections: the set of expanded
// file paths (paths are stable across layout toggles and rerenders, unlike
// indices). Pure transitions, kept out of components so they stay testable.

import type { DiffFile } from "../api/types";
import { COMMIT_MSG_PATH } from "../api/types";

/** Fresh per-diff default: everything collapsed except the synthetic
 * commit message — it is the natural entry point for reviewing a commit
 * and the full message lives only there (not in the page header). */
export function defaultExpanded(): ReadonlySet<string> {
  return new Set([COMMIT_MSG_PATH]);
}

/** Returns the input set unchanged (same reference) when already expanded,
 * so callers can setState without a redundant render. */
export function expand(
  cur: ReadonlySet<string>,
  path: string,
): ReadonlySet<string> {
  if (cur.has(path)) return cur;
  const next = new Set(cur);
  next.add(path);
  return next;
}

export function toggle(
  cur: ReadonlySet<string>,
  path: string,
): ReadonlySet<string> {
  const next = new Set(cur);
  if (!next.delete(path)) next.add(path);
  return next;
}

export function expandAll(files: DiffFile[]): ReadonlySet<string> {
  return new Set(files.map((f) => f.path));
}

export function collapseAll(): ReadonlySet<string> {
  return new Set();
}

/** True when every file of the diff is expanded (drives the rail's
 * expand-all ⇄ collapse-all toggle; an empty diff is never "all"). */
export function allExpanded(
  cur: ReadonlySet<string>,
  files: DiffFile[],
): boolean {
  return files.length > 0 && files.every((f) => cur.has(f.path));
}
