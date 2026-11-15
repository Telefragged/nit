// Change versions: a revision, or the base of a revision.

import type { ChangeVersion } from "../api/types";

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
