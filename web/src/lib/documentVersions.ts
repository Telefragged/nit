import type { Run, Version } from "../api/document.gen";

/** The version of a document with no edit. */
export const EMPTY: Version = "-";

/** Two versions of a document to compare, as indexes of its runs. */
export interface VersionRange {
  /** The run whose version the diff ends at. */
  to: number;
  /** The run whose version the diff starts at, or null for the empty
   * text. */
  from: number | null;
}

/** The range that the `version` and `against` parameters of the page
 * select, or null when `version` is the version of no run. Without a
 * valid `against`, the diff starts at the run before `version`, so it
 * shows what that run changed. */
export function versionRange(
  runs: readonly Run[],
  version: string | null,
  against: string | null,
): VersionRange | null {
  const to = runs.findIndex((run) => run.version === version);
  if (to < 0) return null;
  if (against === EMPTY) return { to, from: null };
  const from = runs.findIndex((run) => run.version === against);
  if (from >= 0 && from < to) return { to, from };
  return runRange(to);
}

/** The range that shows what run `to` changed: the diff from the run
 * before it, or from the empty text for the first run. */
export const runRange = (to: number): VersionRange => ({
  to,
  from: to > 0 ? to - 1 : null,
});

/** The version that `range` starts at. Undefined when `runs` lacks the
 * run. */
export const fromVersion = (
  runs: readonly Run[],
  range: VersionRange,
): Version | undefined =>
  range.from === null ? EMPTY : runs[range.from]?.version;

/** The `version` and `against` parameters of the page for `range`, as a
 * patch of the query. Null selects the live document. A range that
 * starts at the run before its version has no `against`. */
export function rangeParams(
  runs: readonly Run[],
  range: VersionRange | null,
): { version: string | null; against: string | null } {
  const to = range && runs[range.to];
  if (!range || !to) return { version: null, against: null };
  const from = fromVersion(runs, range);
  const before = fromVersion(runs, runRange(range.to));
  return {
    version: to.version,
    against: from === undefined || from === before ? null : from,
  };
}

/** The label of run `index`, as `r0` is the label of a revision. */
export const versionLabel = (index: number) => `v${index}`;
