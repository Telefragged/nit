import { createContext, useContext } from "react";
import type { ChainRef, CommentRange, CommentSide } from "../api/types";

/** Anchor of the draft editor currently open in the diff. */
export interface DraftTarget {
  file: string;
  side: CommentSide;
  /** The range's end line when a range is set. */
  line: number;
  /** Selected-text anchor (docs/api.md "Range comments"). */
  range?: CommentRange;
}

const sameRange = (a?: CommentRange, b?: CommentRange) =>
  a === undefined || b === undefined
    ? a === b
    : a.start_line === b.start_line &&
      a.start_char === b.start_char &&
      a.end_line === b.end_line &&
      a.end_char === b.end_char;

/** Whole-anchor equality — a same-line target with a different range is a
 * different target (the editor must re-anchor). */
export const sameTarget = (a: DraftTarget, b: DraftTarget) =>
  a.file === b.file &&
  a.side === b.side &&
  a.line === b.line &&
  sameRange(a.range, b.range);

export interface ReviewCtx {
  changeId: number;
  /** The TO revision (right select) — the diff's new column. New-column
   * drafts anchor here, and comments place against this range
   * (docs/api.md "Comment placement"). */
  selected: number;
  /** The FROM side: undefined = base, else the interdiff's left revision. */
  against: number | undefined;
  /** Every tip walking through this change, each pinning a patchset — the
   * chain context for the viewed revision is the ref whose `revision`
   * matches `selected`. */
  chains: ChainRef[];
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
