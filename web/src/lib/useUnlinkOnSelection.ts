import { type RefObject, useEffect } from "react";

export function useUnlinkOnSelection(
  root: RefObject<Element | null>,
  linked: boolean,
  unlink: () => void,
) {
  useEffect(() => {
    if (!linked) return;
    const onChange = () => {
      const sel = document.getSelection();
      if (!sel?.anchorNode || sel.isCollapsed) return;
      if (root.current?.contains(sel.anchorNode)) return;
      unlink();
    };
    document.addEventListener("selectionchange", onChange);
    return () => {
      document.removeEventListener("selectionchange", onChange);
    };
  }, [root, linked, unlink]);
}
