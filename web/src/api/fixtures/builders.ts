// Pure fixture builders: deterministic shas, relative timestamps, diff-line
// constructors, and the small diff helpers the fixture records lean on. No
// state lives here — see ./data for the mutable store, ./index for the server.

import { COMMIT_MSG_PATH } from "../types";
import type { Diff, DiffFile, Line } from "../types";

const NOW = Date.now();
export const ago = (minutes: number) =>
  new Date(NOW - minutes * 60_000).toISOString();

/** Deterministic fake 40-hex sha from a numeric seed. */
export function sha(seed: number): string {
  let x = (seed * 2654435761) >>> 0;
  let out = "";
  for (let i = 0; i < 40; i++) {
    x = (x * 1103515245 + 12345) >>> 0;
    out += ((x >>> 16) % 16).toString(16);
  }
  return out;
}

export const ctx = (old: number, nw: number, text: string): Line => ({
  kind: "context",
  old,
  new: nw,
  text,
});
export const add = (nw: number, text: string): Line => ({
  kind: "add",
  new: nw,
  text,
});
export const del = (old: number, text: string): Line => ({
  kind: "del",
  old,
  text,
});
/** Mark a line as rebase drift (docs/api.md "Rebase-aware interdiffs"). */
export const drift = (line: Line): Line => ({ ...line, drift: true });

/** The /COMMIT_MSG entry of a vs-parent diff: the whole message, all-add. */
export function msgFile(message: string): DiffFile {
  const lines = message.replace(/\n$/, "").split("\n");
  return {
    path: COMMIT_MSG_PATH,
    status: "added",
    binary: false,
    additions: lines.length,
    deletions: 0,
    hunks: [
      {
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines: lines.length,
        header: "",
        lines: lines.map((text, i) => add(i + 1, text)),
      },
    ],
  };
}

export const diffKey = (revision: number, against?: number) =>
  against === undefined ? `r${revision}` : `r${against}..r${revision}`;

export function trivialDiff(message: string, path: string, line: string): Diff {
  return {
    files: [
      msgFile(message),
      {
        path,
        status: "modified",
        binary: false,
        additions: 1,
        deletions: 0,
        hunks: [
          {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            header: "",
            lines: [ctx(1, 1, "// orbit"), add(2, line)],
          },
        ],
      },
    ],
  };
}
