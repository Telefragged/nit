// Maps a DOM selection inside a diff to a draft target with a comment
// range, gerrit-style. The DOM contract is
// rendered by DiffFileView: every file section has `data-diff-path`,
// every commentable code cell is a `.code` with `data-old`/`data-new`
// line-number attributes (whichever sides exist for that cell), and the
// line's text — sign and gutters excluded — lives inside a `.code-text`
// span within the cell. A block of a rendered Markdown file is a
// `.doc-block` with `data-side`. The block, and each element inside it
// that a comment can anchor to, is a part: it has `data-part`, its
// `data-start` and `data-end` lines, and `data-end-char`, the length of
// its last line. A table cell also has `data-chars`, the range of its
// content's characters.

import type { LineAnchor, Side } from "../api/types";
import type { Lines } from "./diffLink";
import type { DraftTarget } from "../pages/reviewContext";

/** `Range.intersectsNode`, hand-rolled so jsdom tests and the browser run
 * the same code: overlap is strict (touching at a boundary is not
 * intersecting). */
function intersects(range: Range, node: Node): boolean {
  const r = (node.ownerDocument ?? document).createRange();
  r.selectNode(node);
  return (
    range.compareBoundaryPoints(Range.END_TO_START, r) < 0 &&
    range.compareBoundaryPoints(Range.START_TO_END, r) > 0
  );
}

/** The element that `node` is, or the element that contains it. */
const elementOf = (node: Node) =>
  node instanceof Element ? node : node.parentElement;

const cellOf = (node: Node): HTMLElement | null =>
  elementOf(node)?.closest(".code") ?? null;

/** The path of the file whose section contains `el`. */
const pathOf = (el: Element | null | undefined) =>
  el?.closest("section[data-diff-path]")?.getAttribute("data-diff-path");

/** The section of `file`. */
const sectionOf = (file: string) =>
  [...document.querySelectorAll("section[data-diff-path]")].find(
    (s) => s.getAttribute("data-diff-path") === file,
  );

/** The rendered Markdown block that contains `node`, when it comes from
 * source lines. */
const blockOf = (node: Node) =>
  elementOf(node)?.closest<HTMLElement>(".doc-block[data-part]");

/** The smallest part of a rendered Markdown block that contains `node`:
 * the block, or an element inside it that a comment can anchor to. */
const partOf = (node: Node) =>
  elementOf(node)?.closest<HTMLElement>("[data-part]");

/** The anchor of a comment on whole lines `start` through `end`: one line
 * whole, or several from the start of the first to `endChar`, the length
 * of the last. */
const linesAnchor = (
  start: number,
  end: number,
  endChar: number,
): LineAnchor =>
  start === end
    ? { whole: start }
    : {
        selection: {
          start_line: start,
          start_char: 0,
          end_line: end,
          end_char: endChar,
        },
      };

/** The draft target on parts `first` through `last` of a rendered
 * Markdown file: their whole lines, or the characters of one table cell.
 * Null when the two lie in different files or on different sides. */
function partsTarget(
  first: HTMLElement,
  last: HTMLElement,
): DraftTarget | null {
  const file = pathOf(first);
  const side = selectionAnchorSide(first);
  if (!file || side === null) return null;
  if (pathOf(last) !== file || selectionAnchorSide(last) !== side) return null;
  const start = Number(first.dataset.start);
  const end = Number(last.dataset.end);
  const chars = first === last ? first.dataset.chars?.split("-") : undefined;
  const at: LineAnchor = chars
    ? {
        selection: {
          start_line: start,
          start_char: Number(chars[0]),
          end_line: end,
          end_char: Number(chars[1]),
        },
      }
    : linesAnchor(start, end, Number(last.dataset.endChar));
  return { file, side, at };
}

/** The code-text span of a cell — null also for an *empty* line, whose
 * span contains only the zero-width-space row-height placeholder: it is not
 * line text and must count as length 0, or selections ending past an
 * empty line would anchor a char that does not exist. */
function codeTextOf(cell: Element): Element | null {
  const code = cell.querySelector(".code-text");
  return code?.textContent === "​" ? null : code;
}

const cellTextLength = (cell: Element): number =>
  codeTextOf(cell)?.textContent.length ?? 0;

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
  const within = root.closest(".code");
  const candidates = within
    ? [within]
    : Array.from(root.querySelectorAll(".code"));
  return candidates.filter((c): c is HTMLElement => intersects(range, c));
}

/** The side both boundary cells can express, preferring "new" (the
 * post-change side). Null when the boundaries disagree — the selected
 * text is not contiguous on either side. */
function sideOf(first: HTMLElement, last: HTMLElement): Side | null {
  if (first.dataset.new !== undefined && last.dataset.new !== undefined) {
    return "new";
  }
  if (first.dataset.old !== undefined && last.dataset.old !== undefined) {
    return "old";
  }
  return null;
}

/** The line a code cell shows, with its file and side. A unified cell
 * that shows both sides reads as the new side. Null when `node` is not in
 * a code cell that shows a line. */
export function cellLine(
  node: Element,
): { file: string; side: Side; line: number } | null {
  const cell = cellOf(node);
  const file = pathOf(cell);
  if (!cell || !file) return null;
  const side: Side = cell.dataset.new === undefined ? "old" : "new";
  const line = Number(cell.dataset[side]);
  return Number.isInteger(line) && line > 0 ? { file, side, line } : null;
}

/** The lines on `side` of the row that contains `el`, from the data
 * attributes of the row's line number (components/diff/LineLink): one line
 * of the diff, or the lines of a rendered Markdown block. Null outside a
 * row with a line number on that side. */
export function rowLines(el: Element, side: Side): Lines | null {
  const number = el
    .closest(".line-row, .doc-block")
    ?.querySelector<HTMLElement>(`.line-link[data-side="${side}"]`);
  return number
    ? [Number(number.dataset.start), Number(number.dataset.end)]
    : null;
}

/** A range over lines `start` through `end` on one side of `file`'s
 * section: from the start of the first line to the end of the last. In a
 * rendered Markdown file, it covers the part that spans exactly those
 * lines, or else runs from the block that starts on the first line to the
 * block that ends on the last. Null when either end does not render. */
export function linesRange(
  file: string,
  side: Side,
  start: number,
  end: number,
): Range | null {
  const section = sectionOf(file);
  const block = `.doc-block[data-side="${side}"]`;
  const lines = `[data-start="${start}"][data-end="${end}"]`;
  const part = section?.querySelector(
    `${block}${lines}, ${block} [data-part]${lines}:not([data-chars])`,
  );
  const first =
    part ??
    section?.querySelector(
      `.code[data-${side}="${start}"], ${block}[data-start="${start}"]`,
    );
  const last =
    part ??
    section?.querySelector(
      `.code[data-${side}="${end}"], ${block}[data-end="${end}"]`,
    );
  if (!first || !last) return null;
  const range = document.createRange();
  range.setStart(first, 0);
  range.setEnd(last, last.childNodes.length);
  return range;
}

/** Why a selection inside the diff maps to no target — surfaced to the
 * user by the `c` handler. Selections with no commentable cells at all
 * (outside the diff) return null instead and stay silent. */
export interface SelectionMiss {
  miss: "mixed-sides" | "cross-file" | "hunk-gap";
}

/**
 * Which split-view column a live selection visually belongs to — the side
 * of the cell its anchor (where the drag began) sits in. `null` when the
 * anchor is outside a split cell: a unified diff (cells have no
 * `data-side`), a collapsed caret, or a sweep that started off the diff (a
 * select-all). ReviewPage mirrors this onto the diff column so the CSS can
 * blank the *other* column's selection paint — the interleaved subgrid
 * makes a one-side range sweep both columns' cells, but only the dragged
 * side should highlight.
 */
export function selectionAnchorSide(anchor: Node | null): Side | null {
  const el =
    anchor instanceof Element ? anchor : (anchor?.parentElement ?? null);
  const side = el?.closest("[data-side]")?.getAttribute("data-side");
  return side === "old" || side === "new" ? side : null;
}

/**
 * The draft target a selection produces; a [`SelectionMiss`] when the
 * selection touches the diff but maps to no commentable range (sides
 * that disagree, lines that are not consecutive on the chosen side, a
 * sweep across file sections); null when it has nothing to do with the
 * diff.
 *
 * Either column is commentable: the old column reports `side: "old"`, the
 * new column `side: "new"`; the caller maps that to a stored
 * (revision, side) for the current diff range (lib/comments). In split
 * view the swept DOM region covers both columns; cells of the other side
 * are dropped here, so a one-column drag (which styles/diff.css `sel-old`/
 * `sel-new` also bias toward where the engine honors user-select) maps to
 * that column's contiguous lines. A collapsed
 * selection inside a single cell degrades to a plain line comment on that
 * cell — gerrit's `c`-on-a-line. A selection ending before a line's first
 * character ends on the previous line. In a rendered Markdown file, a
 * selection inside one block drafts on the smallest part that contains
 * all of it. A selection over several blocks drafts on their whole lines, from
 * the first block it touches to the last.
 *
 * `side` overrides the side that `sideOf` reads from the boundary cells.
 * Where both boundary cells show both sides, `sideOf` reads the new side.
 */
export function selectionTarget(
  range: Range,
  side?: Side,
): DraftTarget | SelectionMiss | null {
  const swept = sweptCells(range);
  const firstSwept = swept[0];
  const lastSwept = swept[swept.length - 1];
  if (firstSwept === undefined || lastSwept === undefined) {
    const first = blockOf(range.startContainer);
    const last = blockOf(range.endContainer);
    if (!first || !last) return null;
    if (first !== last) return partsTarget(first, last);
    const part = partOf(range.commonAncestorContainer);
    return part ? partsTarget(part, part) : null;
  }
  const startCell = cellOf(range.startContainer) ?? firstSwept;
  const endCell = cellOf(range.endContainer) ?? lastSwept;

  side ??= sideOf(startCell, endCell) ?? undefined;
  if (side === undefined) return { miss: "mixed-sides" };

  const cells = swept.filter((c) => c.dataset[side] !== undefined);
  const firstCell = cells[0];
  const lastCell = cells[cells.length - 1];
  if (firstCell === undefined || lastCell === undefined) return null;

  const section = firstCell.closest("section[data-diff-path]");
  const path = section?.getAttribute("data-diff-path");
  if (!path) return null;
  if (lastCell.closest("section") !== section) {
    return { miss: "cross-file" };
  }

  let startChar =
    startCell === firstCell
      ? boundaryChar(startCell, range.startContainer, range.startOffset)
      : 0;
  let endChar =
    endCell === lastCell
      ? boundaryChar(endCell, range.endContainer, range.endOffset)
      : cellTextLength(lastCell);

  // A selection reaching a line but owning none of its text ends on the
  // previous line (triple-click and drag-past-end both land here).
  while (cells.length > 1 && endChar === 0) {
    cells.pop();
    const prev = cells[cells.length - 1];
    if (prev === undefined) break;
    endChar = cellTextLength(prev);
  }

  const lines = cells.map((c) => Number(c.dataset[side]));
  if (lines.some((n) => !Number.isInteger(n) || n < 1)) return null;
  for (let i = 1; i < lines.length; i++) {
    const prev = lines[i - 1];
    const cur = lines[i];
    if (prev === undefined || cur === undefined) continue;
    if (cur !== prev + 1) return { miss: "hunk-gap" };
  }

  const startLine = lines[0];
  const line = lines[lines.length - 1];
  if (startLine === undefined || line === undefined) return null;
  if (cells.length === 1) {
    startChar = Math.min(startChar, endChar);
    if (startChar === endChar) {
      return { file: path, side, at: { whole: line } };
    }
  }
  return {
    file: path,
    side,
    at: {
      selection: {
        start_line: startLine,
        start_char: startChar,
        end_line: line,
        end_char: endChar,
      },
    },
  };
}
