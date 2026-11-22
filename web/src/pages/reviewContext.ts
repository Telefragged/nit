import { createContext, useContext } from "react";
import type { Side, ChangeVersion, Whitespace } from "../api/types";
import type { DiffLink } from "../lib/diffLink";
import {
  placementLine,
  rangeOf,
  type DiffRange,
  type LineOrRange,
} from "../lib/comments";

/** Anchor of the draft editor currently open in the diff. */
export interface DraftTarget {
  file: string;
  side: Side;
  at: LineOrRange;
}

/** The line a target hangs under. A selection ends on that line. */
export const targetLine = (t: DraftTarget) => placementLine(t.at);

/** The selection a target holds, if it holds one. */
export const targetRange = (t: DraftTarget) => rangeOf(t.at);

const sameAt = (a: LineOrRange, b: LineOrRange) =>
  "line" in a
    ? "line" in b && a.line.line === b.line.line
    : "range" in b &&
      a.range.start_line === b.range.start_line &&
      a.range.start_char === b.range.start_char &&
      a.range.end_line === b.range.end_line &&
      a.range.end_char === b.range.end_char;

/** Whole-anchor equality. A same-line target with a different selection
 * is a different target, so the editor re-anchors. */
export const sameTarget = (a: DraftTarget, b: DraftTarget) =>
  a.file === b.file && a.side === b.side && sameAt(a.at, b.at);

export interface ReviewCtx {
  changeNumber: number;
  /** The TO revision (right select) — the diff's new column. New-column
   * drafts anchor here, and comments place against this range. */
  selected: number;
  /** The FROM side: the base, or the interdiff's left revision. */
  against: ChangeVersion;
  /** The change's newest revision. */
  latestRevision: number;
  /** How the diff on screen compares whitespace. A reveal reads the file
   * the same way, so a revealed line matches the hunks around it. */
  whitespace: Whitespace;
  /** The hashes of the attachments that the two revisions show. An attach
   * entry changes it, and the attachments' lines with it. */
  attachKey: string;
  /** Shows a diff range the page computed for a thread (`rangeSince`). It
   * runs the same draft-editor guard as the range selects. */
  showRange: (range: DiffRange) => void;
  /** The place the page's URL links to, or null. */
  link: DiffLink | null;
  /** The URL of this page with `link` in place of the current one. */
  linkHref: (link: DiffLink) => string;
  /** Writes `link` into the URL without moving the view. */
  selectLink: (link: DiffLink) => void;
  editingTarget: DraftTarget | null;
  /** Guarded: moving or clearing the target unmounts the inline editor, so
   * this confirms first while `editorDirty` is set, returning whether the
   * move was applied. Same-anchor calls are no-ops (the editor stays
   * mounted). */
  setEditingTarget: (t: DraftTarget | null) => boolean;
  /** Record whether the inline draft editor contains unsaved text (the
   * editor reports it via onDirtyChange). */
  setEditorDirty: (dirty: boolean) => void;
}

/** The location state of a link that moves the view to its place, even
 * when the page already shows that link. */
export const REVEAL = "reveal";

export const ReviewContext = createContext<ReviewCtx | null>(null);

export function useReview(): ReviewCtx {
  const ctx = useContext(ReviewContext);
  if (!ctx) throw new Error("useReview outside ReviewContext");
  return ctx;
}
