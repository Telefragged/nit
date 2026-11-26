import type { Run } from "../api/document.gen";

/** The versions of a document that the page shows, as indexes of its
 * runs. With `from` equal to `to`, the page shows the text at that
 * version. Otherwise it shows the diff from the text at `from` to the
 * text at `to`. */
export interface VersionRange {
  from: number;
  to: number;
}

/** The range that shows the text at run `index`. */
export const oneVersion = (index: number): VersionRange => ({
  from: index,
  to: index,
});

/** The range that the `version` and `against` parameters of the page
 * select, or null when `version` is the version of no run. Without an
 * `against` that is the version of an earlier run, the range is the one
 * version. */
export function versionRange(
  runs: readonly Run[],
  version: string | null,
  against: string | null,
): VersionRange | null {
  const to = runs.findIndex((run) => run.version === version);
  if (to < 0) return null;
  const from = runs.findIndex((run) => run.version === against);
  return from >= 0 && from < to ? { from, to } : oneVersion(to);
}

/** `range` with its end moved to run `index`, or with its start moved
 * there when the run is before the range. Without a range, the result
 * is the one version. */
export function extendedRange(
  range: VersionRange | null,
  index: number,
): VersionRange {
  if (!range) return oneVersion(index);
  return index < range.from
    ? { from: index, to: range.to }
    : { from: range.from, to: index };
}

/** The `version` and `against` parameters of the page for `range`, as a
 * patch of the query. Null selects the live document. A range of one
 * version has no `against`. */
export function rangeParams(
  runs: readonly Run[],
  range: VersionRange | null,
): { version: string | null; against: string | null } {
  const to = range && runs[range.to];
  if (!range || !to) return { version: null, against: null };
  return {
    version: to.version,
    against:
      range.from === range.to ? null : (runs[range.from]?.version ?? null),
  };
}

/** What the page shows for `range`, in words. */
export const rangeLabel = ({ from, to }: VersionRange) =>
  from === to ? `Version ${to}` : `Versions ${from} to ${to}`;

/** The label of run `index`, as `r0` is the label of a revision. */
export const versionLabel = (index: number) => `v${index}`;
