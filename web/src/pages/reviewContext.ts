import { createContext, useContext } from "react";
import type { LineAnchor, Side } from "../api/types";
import { placementLine } from "../lib/comments";

/** Anchor of the draft editor currently open in the diff. */
export interface DraftTarget {
  file: string;
  side: Side;
  /** The whole line, or the selection inside it. */
  at: LineAnchor;
}

/** The line a target hangs under. A selection ends on that line. */
export const targetLine = (t: DraftTarget) => placementLine(t.at);

/** The selection a target holds, if it holds one. */
export const targetRange = (t: DraftTarget) =>
  "selection" in t.at ? t.at.selection : null;

const sameAt = (a: LineAnchor, b: LineAnchor) =>
  "whole" in a
    ? "whole" in b && a.whole === b.whole
    : "selection" in b &&
      a.selection.start_line === b.selection.start_line &&
      a.selection.start_char === b.selection.start_char &&
      a.selection.end_line === b.selection.end_line &&
      a.selection.end_char === b.selection.end_char;

/** Whole-anchor equality. A same-line target with a different selection
 * is a different target, so the editor re-anchors. */
export const sameTarget = (a: DraftTarget, b: DraftTarget) =>
  a.file === b.file && a.side === b.side && sameAt(a.at, b.at);

export interface ReviewCtx {
  changeNumber: number;
  /** The TO revision (right select) — the diff's new column. New-column
   * drafts anchor here, and comments place against this range. */
  selected: number;
  /** The FROM side: undefined = base, else the interdiff's left revision. */
  against: number | undefined;
  editingTarget: DraftTarget | null;
  /** Guarded: moving or clearing the target unmounts the inline editor, so
   * this confirms first while `editorDirty` is set, returning whether the
   * move was applied. Same-anchor calls are no-ops (the editor stays
   * mounted). */
  setEditingTarget: (t: DraftTarget | null) => boolean;
  /** Record whether the inline draft editor holds unsaved text (the editor
   * reports it via onDirtyChange). The provider owns the backing ref, so the
   * mutation lives where the ref is constructed. */
  setEditorDirty: (dirty: boolean) => void;
}

export const ReviewContext = createContext<ReviewCtx | null>(null);

export function useReview(): ReviewCtx {
  const ctx = useContext(ReviewContext);
  if (!ctx) throw new Error("useReview outside ReviewContext");
  return ctx;
}
