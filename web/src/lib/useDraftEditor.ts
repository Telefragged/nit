import { useCallback, useRef, useState } from "react";
import { confirmDiscard } from "./confirmDiscard";
import {
  sameTarget,
  targetLine,
  type DraftTarget,
  type ReviewCtx,
} from "./reviewContext";

/** The draft editor of a page: the anchor of the one editor that is open
 * in a diff, and the guard on its unsaved text.
 *
 * The result has the three editor fields of `ReviewCtx`. */
export function useDraftEditor(): Pick<
  ReviewCtx,
  "editingTarget" | "setEditingTarget" | "setEditorDirty"
> {
  const [editingTarget, setTarget] = useState<DraftTarget | null>(null);
  const dirty = useRef(false);
  const setEditingTarget = useCallback(
    (t: DraftTarget | null) => {
      const cur = editingTarget;
      if (t && cur && sameTarget(t, cur)) return true;
      const sameCell =
        t !== null &&
        cur !== null &&
        t.file === cur.file &&
        t.side === cur.side &&
        targetLine(t) === targetLine(cur);
      if (!sameCell && !confirmDiscard(dirty.current)) return false;
      setTarget(t);
      return true;
    },
    [editingTarget],
  );
  const setEditorDirty = useCallback((next: boolean) => {
    dirty.current = next;
  }, []);
  return { editingTarget, setEditingTarget, setEditorDirty };
}
