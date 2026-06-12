// Maps a DOM selection inside a diff to a draft target with a comment
// range (docs/api.md "Range comments"), gerrit-style. The DOM contract is
// rendered by DiffFileView: every file section carries `data-diff-path`,
// every commentable code cell is a `td.code` with `data-old`/`data-new`
// line-number attributes (whichever sides exist for that cell), and the
// line's text — sign and gutters excluded — lives inside a `.code-text`
// span within the cell.

import type { CommentSide } from "../api/types";
import type { DraftTarget } from "../pages/reviewContext";

/** `Range.intersectsNode`, hand-rolled so jsdom tests and the browser run
 * the same code: overlap is strict (touching at a boundary is not
 * intersecting). */
function intersects(range: Range, node: Node): boolean {
  const r = node.ownerDocument!.createRange();
  r.selectNode(node);
  return (
    range.compareBoundaryPoints(Range.END_TO_START, r) < 0 &&
    range.compareBoundaryPoints(Range.START_TO_END, r) > 0
  );
}

const cellOf = (node: Node): HTMLElement | null =>
  (node instanceof Element ? node : node.parentElement)?.closest("td.code") ??
  null;

/** The code-text span of a cell — null also for an *empty* line, whose
 * span holds only the zero-width-space row-height placeholder: it is not
 * line text and must count as length 0, or selections ending past an
 * empty line would anchor a char that does not exist. */
function codeTextOf(cell: Element): Element | null {
  const code = cell.querySelector(".code-text");
  return code?.textContent === "​" ? null : code;
}

const cellTextLength = (cell: Element): number =>
  codeTextOf(cell)?.textContent?.length ?? 0;

/** Char offset of a boundary point within `cell`'s code text. Points
 * outside the `.code-text` span (the sign, the cell itself) clamp to the
 * nearest edge. */
function boundaryChar(cell: Element, node: Node, offset: number): number {
  const code = codeTextOf(cell);
  if (!code) return 0;
  const doc = code.ownerDocument;
  const whole = doc.createRange();
  whole.selectNodeContents(code);
  const point = doc.createRange();
  point.setStart(node, offset);
  point.collapse(true);
  if (point.compareBoundaryPoints(Range.START_TO_START, whole) < 0) return 0;
  if (point.compareBoundaryPoints(Range.END_TO_END, whole) > 0) {
    return whole.toString().length;
  }
  const prefix = doc.createRange();
  prefix.setStart(whole.startContainer, whole.startOffset);
  prefix.setEnd(node, offset);
  return prefix.toString().length;
}

/** Every code cell the range sweeps, in document order — both sides; the
 * caller narrows to one. */
function sweptCells(range: Range): HTMLElement[] {
  const node = range.commonAncestorContainer;
  const root = node instanceof Element ? node : node.parentElement;
  if (!root) return [];
  const within = root.closest("td.code");
  const candidates = within
    ? [within]
    : Array.from(root.querySelectorAll("td.code"));
  return candidates.filter((c): c is HTMLElement => intersects(range, c));
}

/** The side both boundary cells can express, preferring "new" (the side a
 * plain click would comment on). Null when the boundaries disagree — the
 * selected text is not contiguous on either side. */
function sideOf(first: HTMLElement, last: HTMLElement): CommentSide | null {
  if (first.dataset["new"] !== undefined && last.dataset["new"] !== undefined) {
    return "new";
  }
  if (first.dataset["old"] !== undefined && last.dataset["old"] !== undefined) {
    return "old";
  }
  return null;
}

/** Why a selection inside the diff maps to no target — surfaced to the
 * user by the `c` handler. Selections with no commentable cells at all
 * (outside the diff) return null instead and stay silent. */
export interface SelectionMiss {
  miss: "mixed-sides" | "old-side-interdiff" | "cross-file" | "hunk-gap";
}

/**
 * The draft target a selection produces; a [`SelectionMiss`] when the
 * selection touches the diff but maps to no commentable range (sides
 * that disagree, lines that are not consecutive on the chosen side, the
 * old side of an interdiff, a sweep across file sections); null when it
 * has nothing to do with the diff.
 *
 * In split view the swept DOM region covers both columns; cells of the
 * other side are dropped, so a one-column drag (styles.css `sel-old`/
 * `sel-new` enforce this) maps to that column's contiguous lines. A
 * collapsed selection inside a single cell degrades to a plain line
 * comment on that cell — gerrit's `c`-on-a-line. A selection ending
 * before a line's first character ends on the previous line.
 */
export function selectionTarget(
  range: Range,
  interdiff: boolean,
): DraftTarget | SelectionMiss | null {
  const swept = sweptCells(range);
  if (swept.length === 0) return null;
  const startCell = cellOf(range.startContainer) ?? swept[0]!;
  const endCell = cellOf(range.endContainer) ?? swept[swept.length - 1]!;

  const side = sideOf(startCell, endCell);
  if (side === null) return { miss: "mixed-sides" };
  if (side === "old" && interdiff) {
    return { miss: "old-side-interdiff" }; // docs/api.md: new side only
  }

  const cells = swept.filter((c) => c.dataset[side] !== undefined);
  if (cells.length === 0) return null;

  const section = cells[0]!.closest("section[data-diff-path]");
  const path = section?.getAttribute("data-diff-path");
  if (!path) return null;
  if (cells[cells.length - 1]!.closest("section") !== section) {
    return { miss: "cross-file" };
  }

  let startChar =
    startCell === cells[0]
      ? boundaryChar(startCell, range.startContainer, range.startOffset)
      : 0;
  let endChar =
    endCell === cells[cells.length - 1]
      ? boundaryChar(endCell, range.endContainer, range.endOffset)
      : cellTextLength(cells[cells.length - 1]!);

  // A selection reaching a line but owning none of its text ends on the
  // previous line (triple-click and drag-past-end both land here).
  while (cells.length > 1 && endChar === 0) {
    cells.pop();
    endChar = cellTextLength(cells[cells.length - 1]!);
  }

  const lines = cells.map((c) => Number(c.dataset[side]));
  if (lines.some((n) => !Number.isInteger(n) || n < 1)) return null;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i] !== lines[i - 1]! + 1) return { miss: "hunk-gap" };
  }

  const line = lines[lines.length - 1]!;
  if (cells.length === 1) {
    startChar = Math.min(startChar, endChar);
    if (startChar === endChar) {
      return { file: path, side, line }; // collapsed: plain line comment
    }
  }
  return {
    file: path,
    side,
    line,
    range: {
      start_line: lines[0]!,
      start_char: startChar,
      end_line: line,
      end_char: endChar,
    },
  };
}
