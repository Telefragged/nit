import {
  type ReactNode,
  type RefObject,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { fileLink, pressLink, type LinkSpan } from "../../lib/diffLink";
import { type DraftTarget, useReview } from "../../lib/reviewContext";
import type { SelectionMiss } from "../../lib/selection";
import {
  cellLine,
  linesRange,
  partLink,
  selectionAnchorSide,
  selectionTarget,
} from "../../lib/selection";
import { plainClick, shortcutKey } from "../../lib/shortcutKey";

const MISS_TEXT: Record<SelectionMiss["miss"], string> = {
  "mixed-sides":
    "A comment can only span one side of the diff. Select from the old or the new side, not both.",
  "cross-file": "A comment can only span one file. Select within a file.",
  "hunk-gap":
    "A comment can only span consecutive lines. This selection jumps a hunk gap.",
};

function missAnchor(range: Range, col: DOMRect): { top: number; left: number } {
  const start =
    range.startContainer instanceof Element
      ? range.startContainer
      : range.startContainer.parentElement;
  const text = start?.closest(".code")?.querySelector(".code-text") ?? null;
  return {
    top: range.getBoundingClientRect().bottom - col.top,
    left: (text?.getBoundingClientRect().left ?? col.left) - col.left,
  };
}

type Column = RefObject<HTMLDivElement | null>;

function useSelectionSideAttribute(column: Column) {
  useEffect(() => {
    const onSelectionChange = () => {
      const col = column.current;
      if (!col) return;
      const sel = document.getSelection();
      const side =
        sel && !sel.isCollapsed && col.contains(sel.anchorNode)
          ? selectionAnchorSide(sel.anchorNode)
          : null;
      // selectionchange fires continuously through a drag. A write of the
      // attribute recalculates the style of the whole diff, so it happens
      // only when the side flips.
      if (side === col.getAttribute("data-sel-side")) return;
      if (side) col.setAttribute("data-sel-side", side);
      else col.removeAttribute("data-sel-side");
    };
    document.addEventListener("selectionchange", onSelectionChange);
    return () => {
      document.removeEventListener("selectionchange", onSelectionChange);
    };
  }, [column]);
}

function useLinkOnLineClick(column: Column) {
  const { link, selectLink } = useReview();
  useEffect(() => {
    const clicked = (e: MouseEvent): LinkSpan | null => {
      const el = e.target;
      if (!(el instanceof Element) || !column.current?.contains(el)) {
        return null;
      }
      const at = cellLine(el);
      return at
        ? {
            file: at.file,
            lines: { side: at.side, start: at.line, end: at.line },
          }
        : partLink(el);
    };
    const onMouseDown = (e: MouseEvent) => {
      if (e.shiftKey && plainClick(e) && clicked(e)) e.preventDefault();
    };
    const onClick = (e: MouseEvent) => {
      const at = clicked(e);
      if (at === null || !plainClick(e)) return;
      if (document.getSelection()?.isCollapsed === false) return;
      const { side, start, end } = at.lines;
      selectLink(
        e.shiftKey
          ? pressLink(link, at.file, side, [start, end], true).span
          : at,
      );
    };
    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("click", onClick);
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("click", onClick);
    };
  }, [column, link, selectLink]);
}

function DraftShortcut({ column }: { column: Column }) {
  const { link, setEditingTarget } = useReview();
  const [selectionMiss, setSelectionMiss] = useState<
    (SelectionMiss & { top: number; left: number }) | null
  >(null);

  useLayoutEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const col = column.current;
      if (shortcutKey(e) !== "c" || col === null) return;
      const sel = document.getSelection();
      const linked = fileLink(link);
      let range: Range | null = null;
      let result: DraftTarget | SelectionMiss | null = null;
      if (sel && !sel.isCollapsed && sel.rangeCount > 0) {
        range = sel.getRangeAt(0);
        if (!col.contains(range.commonAncestorContainer)) return;
        result = selectionTarget(range);
      } else if (linked?.lines) {
        const { side, start, end, chars } = linked.lines;
        if (start === end && !chars) {
          result = { file: linked.file, side, at: { line: start } };
        } else {
          range = linesRange(col, { file: linked.file, lines: linked.lines });
          result = range && selectionTarget(range, side);
        }
      } else if (linked?.rendered) {
        const { side, at } = linked.rendered;
        result = { file: linked.file, side, at };
      }
      if (!result) return;
      // preventDefault, or the keystroke lands in the editor's textarea.
      e.preventDefault();
      if ("miss" in result) {
        if (range) {
          setSelectionMiss({
            ...result,
            ...missAnchor(range, col.getBoundingClientRect()),
          });
        }
        return;
      }
      if (setEditingTarget(result)) sel?.removeAllRanges();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [column, link, setEditingTarget]);

  // A miss describes one selection, so it stays until the reviewer starts
  // another. The event is `selectstart`, not `selectionchange`: the latter
  // also fires for the selection that the miss describes. It is queued as
  // a task, so it can arrive after the keystroke and remove the miss as it
  // appears.
  useEffect(() => {
    const onSelectStart = () => {
      setSelectionMiss(null);
    };
    document.addEventListener("selectstart", onSelectStart);
    return () => {
      document.removeEventListener("selectstart", onSelectStart);
    };
  }, []);

  return selectionMiss ? (
    <div
      className="selection-miss"
      role="status"
      style={{ top: selectionMiss.top, left: selectionMiss.left }}
    >
      {MISS_TEXT[selectionMiss.miss]}
    </div>
  ) : null;
}

/** The column that contains the file sections of one diff, with the
 * gestures that work on every line of it.
 *
 * A click on the code of a line links the line, as a click on its number
 * does. `c` opens the draft editor on the selected text, or else on the
 * linked lines. A selection that no comment can span shows the reason
 * under it.
 *
 * `layout` is the layout of the file sections. `ready` is the diff base
 * of the rendered diff, absent while the diff loads. */
export default function DiffColumn({
  layout,
  ready,
  children,
}: {
  layout: "unified" | "split";
  ready: string | undefined;
  children: ReactNode;
}) {
  const { changeNumber, selected, against, whitespace } = useReview();
  const column = useRef<HTMLDivElement>(null);
  useSelectionSideAttribute(column);
  useLinkOnLineClick(column);
  return (
    <div className="diff-column" ref={column} data-diff-ready={ready}>
      <DraftShortcut
        key={`${changeNumber}:${selected}:${against}:${layout}:${whitespace}`}
        column={column}
      />
      {children}
    </div>
  );
}
