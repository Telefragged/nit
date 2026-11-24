// No state lives here — see ./data for the mutable store, ./index for the server.

import type { ReviewFile } from "../../lib/attachments";
import type { UiThread } from "../../lib/comments";
import { span } from "../../lib/diffview";
import type { ReviewCtx } from "../../lib/reviewContext";
import { ATTACHMENT_PREFIX, COMMIT_MSG_PATH } from "../types";
import type {
  CommentInput,
  Diff,
  DiffFile,
  Draft,
  GraphNode,
  Hunk,
  Line,
  Side,
  ThreadComment,
} from "../types";

/** A UI thread with no comments, on line 1 of src/main.rs at revision 1. */
export const uiThread = (
  over: Partial<UiThread> & { id: number | null },
): UiThread => ({
  anchor: {
    file: { version: "r1", path: "src/main.rs", anchor: { line: 1 } },
  },
  resolved: false,
  comments: [],
  drafts: [],
  created_at: "2026-01-01T00:00:00Z",
  ...over,
});

/** The context of a review page at revision 1 of change 1, against the
 * base, with no link and no draft editor. Its callbacks do nothing. */
export const reviewCtx = (over: Partial<ReviewCtx> = {}): ReviewCtx => ({
  changeNumber: 1,
  selected: 1,
  against: "r1^",
  latestRevision: 1,
  whitespace: "compare",
  attachKey: "",
  showRange: () => undefined,
  link: null,
  linkHref: () => "",
  selectLink: () => undefined,
  editingTarget: null,
  setEditingTarget: () => false,
  setEditorDirty: () => undefined,
  ...over,
});

const NOW = Date.now();
export const ago = (minutes: number) =>
  new Date(NOW - minutes * 60_000).toISOString();

export type DraftFields = Partial<Omit<Draft, "comment"> & CommentInput>;

/** A draft on change 1 with no text, written 22 minutes ago. */
export const draft = ({
  id = 1,
  change_number = 1,
  created_at = ago(22),
  updated_at = created_at,
  ...comment
}: DraftFields = {}): Draft => ({
  id,
  change_number,
  comment: {
    thread_id: null,
    revision: 1,
    anchor: "change",
    body: "",
    resolved: null,
    ...comment,
  },
  created_at,
  updated_at,
});

/** A published comment of the reviewer or of the author. */
export const threadComment = (
  minutes: number,
  body: string,
  by: "reviewer" | "author" = "reviewer",
): ThreadComment => ({
  body,
  review_id: by === "reviewer" ? 1 : null,
  created_at: ago(minutes),
});

export function sha(seed: number): string {
  let x = (seed * 2654435761) >>> 0;
  let out = "";
  for (let i = 0; i < 40; i++) {
    x = (x * 1103515245 + 12345) >>> 0;
    out += ((x >>> 16) % 16).toString(16);
  }
  return out;
}

/** A distinct, well-formed object name for `label`.
 *
 * The sibling `sha(seed)` mints one from a number. Tests that name a
 * commit `A` or `c11r3` reach for this one. */
export function shaOf(label: string): string {
  return hexExpansion(label);
}

/** A node of a change graph. `sha` and `parent` are labels for `shaOf`.
 * With a `change`, the node is that open change at revision 1, and
 * pending. Without one, it is a merged commit of the history. */
export const graphNode = ({
  sha,
  parent,
  change = null,
  ...over
}: Partial<GraphNode> & {
  sha: string;
  parent: string;
  change?: number | null;
}): GraphNode => ({
  commit_sha: shaOf(sha),
  section: change === null ? "history" : "open",
  subject: sha,
  status: change === null ? "merged" : "pending",
  parents: [shaOf(parent)],
  change_number: change,
  change_id: change === null ? null : `I${change}`,
  revision: change === null ? null : 1,
  fork_sha: shaOf("a0"),
  group: null,
  ...over,
});

/** A distinct, well-formed `Change-Id` for `label`. */
export function changeId(label: string): string {
  return `I${hexExpansion(label.replace(/^I/, ""))}`;
}

/** `label` as 40 hex characters: itself when it is already hex, its bytes
 * otherwise, zero-padded either way. */
function hexExpansion(label: string): string {
  const hex = /^[0-9a-fA-F]*$/.test(label)
    ? label
    : Array.from(label, (c) =>
        c.charCodeAt(0).toString(16).padStart(2, "0"),
      ).join("");
  return (hex + "0".repeat(40)).slice(0, 40);
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
/** Mark a line as rebase drift. */
export const drift = (line: Line): Line => ({ ...line, drift: true });

/** The line the last hunk reaches on `side`, or the total the file declares
 * for it: the mock's file length, the EOF the trailing expander stops at. */
export function sideEnd(
  file: { hunks: Hunk[]; old_total?: number; new_total?: number },
  side: Side,
): number {
  const declared = side === "old" ? file.old_total : file.new_total;
  const last = file.hunks.at(-1);
  return declared ?? (last ? span(last, side)[1] : 0);
}

const linesOf = (text: string) => text.replace(/\n$/, "").split("\n");

/** The entry of a vs-parent diff for a file outside the tree: the whole
 * text, all-add. */
export function addedFile(path: string, text: string): DiffFile {
  const lines = linesOf(text);
  return {
    path,
    status: "added",
    binary: false,
    additions: lines.length,
    deletions: 0,
    old_total: 0,
    new_total: lines.length,
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

/** The entry of a diff for a file outside the tree, from the text
 * `before` to the text `after`, or deleted without `after`. One hunk spans
 * the whole file, and the lines between the common head and tail change. */
export function changedFile(
  path: string,
  before: string,
  after?: string,
): DiffFile {
  const old = linesOf(before);
  const next = after === undefined ? [] : linesOf(after);
  let head = 0;
  while (head < old.length && head < next.length && old[head] === next[head])
    head++;
  let tail = 0;
  while (
    tail < old.length - head &&
    tail < next.length - head &&
    old[old.length - 1 - tail] === next[next.length - 1 - tail]
  )
    tail++;
  const oldEnd = old.length - tail;
  const nextEnd = next.length - tail;
  return {
    path,
    status: after === undefined ? "deleted" : "modified",
    binary: false,
    additions: nextEnd - head,
    deletions: oldEnd - head,
    old_total: old.length,
    new_total: next.length,
    hunks: [
      {
        old_start: 1,
        old_lines: old.length,
        new_start: next.length === 0 ? 0 : 1,
        new_lines: next.length,
        header: "",
        lines: [
          ...old.slice(0, head).map((text, i) => ctx(i + 1, i + 1, text)),
          ...old.slice(head, oldEnd).map((text, i) => del(head + i + 1, text)),
          ...next
            .slice(head, nextEnd)
            .map((text, i) => add(head + i + 1, text)),
          ...old
            .slice(oldEnd)
            .map((text, i) => ctx(oldEnd + i + 1, nextEnd + i + 1, text)),
        ],
      },
    ],
  };
}

/** The /COMMIT_MSG entry of a vs-parent diff: the whole message, all-add. */
export const msgFile = (message: string) => addedFile(COMMIT_MSG_PATH, message);

/** The review file of the attachment `name` against the parent: its text
 * all-add. */
export const attachedFile = (
  name: string,
  text: string,
  revision = 0,
): ReviewFile => ({
  ...addedFile(ATTACHMENT_PREFIX + name, text),
  attachment: { name, hash: "a".repeat(64), revision },
});

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
        old_total: 1,
        new_total: 2,
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
