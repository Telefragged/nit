// Change versions: a revision, or the base of a revision.

import type { ChangeVersion, Side } from "../api/types";

export const revisionVersion = (revision: number): ChangeVersion =>
  `r${revision}`;

export const baseVersion = (revision: number): ChangeVersion => `r${revision}^`;

/** The revision that `version` is, or is the base of. */
export const revisionOf = (version: ChangeVersion) =>
  Number.parseInt(version.slice(1), 10);

/** Whether `version` is the base of a revision. */
export const isBase = (version: ChangeVersion) => version.endsWith("^");

/** `raw` as a change version, or `null` when it is not one. */
export const parseVersion = (raw: string | null): ChangeVersion | null =>
  raw !== null && /^r\d+\^?$/.test(raw) ? (raw as ChangeVersion) : null;

/** The change version that `side` of revision `revision` shows: the
 * revision, or its base. The inverse of {@link anchorOf}. */
export const sideVersion = (revision: number, side: Side): ChangeVersion =>
  side === "old" ? baseVersion(revision) : revisionVersion(revision);

/** The revision and side that show `version`. */
export const anchorOf = (
  version: ChangeVersion,
): { revision: number; side: Side } => ({
  revision: revisionOf(version),
  side: isBase(version) ? "old" : "new",
});
