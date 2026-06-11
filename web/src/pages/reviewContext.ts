import { createContext, useContext } from "react";
import type { CommentSide } from "../api/types";

/** Anchor of the draft editor currently open in the diff. */
export interface DraftTarget {
  file: string;
  side: CommentSide;
  line: number;
}

export interface ReviewCtx {
  changeId: number;
  /** Revision new drafts anchor to (the "new" side of the current diff). */
  draftRevision: number;
  /** Interdiff view: only new-side lines are commentable (docs/api.md). */
  interdiff: boolean;
  editingTarget: DraftTarget | null;
  setEditingTarget: (t: DraftTarget | null) => void;
}

export const ReviewContext = createContext<ReviewCtx | null>(null);

export function useReview(): ReviewCtx {
  const ctx = useContext(ReviewContext);
  if (!ctx) throw new Error("useReview outside ReviewContext");
  return ctx;
}
