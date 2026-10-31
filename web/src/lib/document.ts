// Splits a Markdown file into the blocks that the review page renders and
// comments on one at a time.

import type { Element, Root, RootContent } from "hast";
import { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import remarkRehype from "remark-rehype";
import { unified } from "unified";
import type { Line, LineAnchor, Side } from "../api/types";
import { sideText } from "./rendered";

/** One block of a rendered Markdown file: a top-level element, or one
 * item of a top-level list. */
export interface Block {
  /** The element to render. An item renders inside a list of its own. */
  node: Element;
  /** The source lines of the block, whole, as a comment anchors to them.
   * Null for an element that no source line holds, such as the list of
   * footnotes. */
  at: LineAnchor | null;
  /** The first and last source line of the block. */
  lines: [number, number];
}

const processor = unified().use(remarkParse).use(remarkGfm).use(remarkRehype);

/** The block elements of `tree`, top-level lists split into their items.
 * remark-rehype puts a newline text node between elements. */
function* blockNodes(tree: Root): Generator<Element> {
  for (const node of tree.children) {
    if (node.type !== "element") continue;
    if (node.tagName !== "ul" && node.tagName !== "ol") {
      yield node;
      continue;
    }
    const items = node.children.filter(
      (c): c is Element => c.type === "element" && c.tagName === "li",
    );
    const first = Number(node.properties.start ?? 1);
    for (const [i, item] of items.entries()) {
      yield {
        ...node,
        position: item.position,
        properties: {
          ...node.properties,
          start: node.tagName === "ol" ? first + i : undefined,
          className: [
            ...[node.properties.className ?? []].flat().map(String),
            ...(i < items.length - 1 ? ["doc-item"] : []),
          ],
        },
        children: [item],
      };
    }
  }
}

/** Passes every URL through react-markdown's filter, which drops a
 * `javascript:` link. Markdown without raw HTML puts a URL only in a
 * link's `href` and an image's `src`. */
function filterUrls(node: Root | RootContent) {
  if (node.type === "element") {
    for (const key of ["href", "src"]) {
      const url = node.properties[key];
      if (url !== undefined)
        node.properties[key] = defaultUrlTransform(String(url));
    }
  }
  if ("children" in node) node.children.forEach(filterUrls);
}

/** Parses `text` into its blocks, in source order. Raw HTML stays out of
 * the result, because the file comes from the author. */
export function parseBlocks(text: string): Block[] {
  const tree = processor.runSync(processor.parse(text));
  filterUrls(tree);
  const lines = text.split("\n");
  const blocks: Block[] = [];
  for (const node of blockNodes(tree)) {
    const pos = node.position;
    const start = pos?.start.line ?? 0;
    let end = pos?.end.line ?? 0;
    // An unclosed code fence runs to the end of the file, past its last
    // line of text.
    while (end > start && !lines[end - 1]) end--;
    const at: LineAnchor | null =
      pos === undefined
        ? null
        : start === end
          ? { whole: start }
          : {
              selection: {
                start_line: start,
                start_char: 0,
                end_line: end,
                end_char: lines[end - 1]?.length ?? 0,
              },
            };
    blocks.push({ node, at, lines: [start, end] });
  }
  return blocks;
}

/** The block that shows a thread on source line `line`: the last block
 * that starts at or before it, or the first block for a line above every
 * block. Undefined when no block has source lines. */
export function blockAt(blocks: readonly Block[], line: number) {
  // A binary search for the first block past `line`. The footnotes, the
  // only blocks without source lines, come last.
  let lo = 0;
  let hi = blocks.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    const b = blocks[mid];
    if (b?.at && b.lines[0] <= line) lo = mid + 1;
    else hi = mid;
  }
  const found = blocks[Math.max(lo - 1, 0)];
  return found?.at ? found : undefined;
}

/** A row of a rendered Markdown file. */
export type DocRow =
  /** A block that the diff leaves as it was, on `side`. `old` is the old
   * side's block with the same lines, when the file has both sides. */
  | { side: Side; same: Block; old?: Block }
  /** A run of changed blocks: the old side's version, then the new
   * side's. */
  | { removed: Block[]; added: Block[] };

/** A Markdown file rendered for the diff on screen. */
export interface RenderedDoc {
  old: Block[];
  new: Block[];
  rows: DocRow[];
}

/** The blocks that `row` shows, each with its side, the old side first. */
export const rowBlocks = (row: DocRow): [Side, Block][] =>
  "same" in row
    ? [
        ...(row.old ? [["old", row.old] satisfies [Side, Block]] : []),
        [row.side, row.same],
      ]
    : [
        ...row.removed.map((b): [Side, Block] => ["old", b]),
        ...row.added.map((b): [Side, Block] => ["new", b]),
      ];

const other = (side: Side): Side => (side === "old" ? "new" : "old");

/** Renders a file from all of its diff lines. A file with one side
 * renders that side, `only`, with no changes. Otherwise a block changes
 * when the diff adds or removes one of its lines, or changes the block
 * that its lines match on the other side. */
export function renderDoc(lines: readonly Line[], only?: Side): RenderedDoc {
  const parse = (side: Side) =>
    only === other(side) ? [] : parseBlocks(sideText(lines, side));
  const doc: RenderedDoc = { old: parse("old"), new: parse("new"), rows: [] };
  if (only) {
    doc.rows = doc[only].map((same) => ({ side: only, same }));
    return doc;
  }

  // Where each line sits in the diff, whether the diff adds or removes
  // it, and the line of context that it matches on the other side.
  const pos = {
    old: new Map<number, number>(),
    new: new Map<number, number>(),
  };
  const edited = { old: new Set<number>(), new: new Set<number>() };
  const match = {
    old: new Map<number, number>(),
    new: new Map<number, number>(),
  };
  for (const [i, line] of lines.entries()) {
    if (line.old !== undefined) pos.old.set(line.old, i);
    if (line.new !== undefined) pos.new.set(line.new, i);
    if (line.kind === "del" && line.old !== undefined) edited.old.add(line.old);
    if (line.kind === "add" && line.new !== undefined) edited.new.add(line.new);
    if (
      line.kind === "context" &&
      line.old !== undefined &&
      line.new !== undefined
    ) {
      match.old.set(line.old, line.new);
      match.new.set(line.new, line.old);
    }
  }
  /** The block whose own lines hold line `n` of `side`. */
  const holder = (side: Side, n: number) => {
    const b = blockAt(doc[side], n);
    return b && n <= b.lines[1] ? b : undefined;
  };
  const editedBlocks = {
    old: new Set([...edited.old].map((n) => holder("old", n))),
    new: new Set([...edited.new].map((n) => holder("new", n))),
  };
  /** The other side's blocks that the lines of `block` match. */
  const partners = (side: Side, { lines: [start, end] }: Block) => {
    const found = new Set<Block>();
    for (let n = start; n <= end; n++) {
      const m = match[side].get(n);
      const b = m === undefined ? undefined : holder(other(side), m);
      if (b) found.add(b);
    }
    return [...found];
  };
  const changed = (side: Side, block: Block) =>
    block.at !== null &&
    (editedBlocks[side].has(block) ||
      partners(side, block).some((b) => editedBlocks[other(side)].has(b)));

  // The new side's blocks, and the old side's changed ones, in diff
  // order. A removed block goes before an added block at the same place.
  const items = [
    ...doc.old
      .filter((block) => changed("old", block))
      .map((block) => ({ side: "old" as const, block })),
    ...doc.new.map((block) => ({ side: "new" as const, block })),
  ].map((item) => ({
    ...item,
    at: pos[item.side].get(item.block.lines[0]) ?? Infinity,
  }));
  items.sort(
    (a, b) =>
      a.at - b.at || (a.side === b.side ? 0 : a.side === "old" ? -1 : 1),
  );

  for (const { side, block } of items) {
    if (side === "new" && !changed("new", block)) {
      doc.rows.push({ side, same: block, old: partners("new", block)[0] });
      continue;
    }
    let run = doc.rows.at(-1);
    if (run === undefined || !("removed" in run)) {
      run = { removed: [], added: [] };
      doc.rows.push(run);
    }
    (side === "old" ? run.removed : run.added).push(block);
  }
  return doc;
}

/** What a rendered diff shows in place of its rows: a row with its
 * index, or a run of hidden rows, `from` through `to`. */
type ShownRow = { index: number; row: DocRow } | { from: number; to: number };

/** The unchanged rows that a rendered diff shows on each side of a
 * change. */
const CONTEXT = 1;

/** The rows of `rows` that a rendered diff shows, in order, with each run
 * of the others hidden in its place.
 *
 * A row shows when it changes, when it is at most `CONTEXT` rows from a
 * change, or when `keep` holds for it. A file with no changed row
 * shows every row. */
export function shownRows(
  rows: readonly DocRow[],
  keep: (row: DocRow, index: number) => boolean,
): ShownRow[] {
  const changed = (row: DocRow) => !("same" in row);
  if (!rows.some(changed)) return rows.map((row, index) => ({ index, row }));
  const near = (i: number) =>
    rows.slice(Math.max(0, i - CONTEXT), i + CONTEXT + 1).some(changed);
  const out: ShownRow[] = [];
  for (const [index, row] of rows.entries()) {
    if (near(index) || keep(row, index)) {
      out.push({ index, row });
      continue;
    }
    const run = out.at(-1);
    if (run && "to" in run) run.to = index;
    else out.push({ from: index, to: index });
  }
  return out;
}
