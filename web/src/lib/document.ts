// Splits a Markdown file into the blocks that the review page renders, and
// tags the parts inside them that a comment can anchor to.

import type { Element, ElementContent, Root, RootContent } from "hast";
import { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import remarkRehype from "remark-rehype";
import { unified } from "unified";
import type { Line, Side } from "../api/types";
import type { Lines } from "./diffLink";
import { sideText } from "./rendered";

/** One block of a rendered Markdown file: a top-level element, or one
 * item of a top-level list. */
export interface Block {
  /** The element to render. An item renders inside a list of its own. */
  node: Element;
  /** Whether the block comes from source lines, so that a comment can
   * anchor to it. False for an element with no source lines, such as the
   * list of footnotes. */
  fromSource: boolean;
  /** The first and last source line of the block. */
  lines: Lines;
  /** The length of the block's last source line, where a comment that
   * ends on the block ends. */
  endChar: number;
  /** The parts that a link can mark: the block itself first, then the
   * parts inside it, in document order. */
  parts: Part[];
}

/** A part of a block that a link or a comment can mark: the block
 * itself, with no `node`, or an element inside it. */
export interface Part {
  node?: Element;
  /** The first and last source line of the part. */
  lines: Lines;
  /** For a table cell, the characters of its content on its line, the
   * end exclusive. */
  chars?: [number, number];
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

/** The elements inside a block that a comment can anchor to on their own,
 * besides the block itself. */
const PARTS = new Set(["li", "tr", "th", "td"]);

/** Sets the data attributes of a part: its source lines as `data-part`,
 * `data-start` and `data-end`, the length of its last line as
 * `data-end-char`, and for a table cell the range of its content's
 * characters as `data-chars` (lib/selection). */
function tag(el: Element, part: Part, lines: readonly string[]) {
  const [start, end] = part.lines;
  el.properties.dataPart = "";
  el.properties.dataStart = start;
  el.properties.dataEnd = end;
  el.properties.dataEndChar = lines[end - 1]?.length ?? 0;
  if (part.chars) el.properties.dataChars = part.chars.join("-");
}

/** Tags each part inside `node`, and adds it to `parts`. The code of a
 * code block becomes one tagged `span.code-line` per line, with the
 * fence's language as `data-lang`. */
function tagParts(node: Element, lines: readonly string[], parts: Part[]) {
  const pos = node.position;
  const code = node.children.find(
    (c): c is Element => c.type === "element" && c.tagName === "code",
  );
  const text = code?.children[0];
  if (pos && node.tagName === "pre" && code && text?.type === "text") {
    // The opening fence is the line above the code. Indented code has
    // no fence.
    const fenced = /^[\s>]*(```|~~~)/.test(lines[pos.start.line - 1] ?? "");
    const first = pos.start.line + (fenced ? 1 : 0);
    const lang = [code.properties.className ?? []]
      .flat()
      .map(String)
      .find((c) => c.startsWith("language-"))
      ?.slice("language-".length);
    code.children = text.value
      .replace(/\n$/, "")
      .split("\n")
      .flatMap((value, i): ElementContent[] => {
        const line: Element = {
          type: "element",
          tagName: "span",
          properties: { className: ["code-line"], dataLang: lang },
          children: [{ type: "text", value }],
        };
        const part = { node: line, lines: [first + i, first + i] as Lines };
        tag(line, part, lines);
        parts.push(part);
        return [line, { type: "text", value: "\n" }];
      });
    return;
  }
  for (const child of node.children) {
    if (child.type !== "element") continue;
    const at = child.position;
    if (at && (PARTS.has(child.tagName) || node.tagName === "blockquote")) {
      const part: Part = { node: child, lines: [at.start.line, at.end.line] };
      if (child.tagName === "th" || child.tagName === "td") {
        // A cell's span starts and ends on its pipes.
        const first = child.children.at(0)?.position?.start ?? at.start;
        const last = child.children.at(-1)?.position?.end ?? at.end;
        part.chars = [first.column - 1, last.column - 1];
      }
      tag(child, part, lines);
      parts.push(part);
    }
    tagParts(child, lines, parts);
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
    const endChar = lines[end - 1]?.length ?? 0;
    const parts: Part[] = [{ lines: [start, end] }];
    tagParts(node, lines, parts);
    blocks.push({
      node,
      fromSource: pos !== undefined,
      lines: [start, end],
      endChar,
      parts,
    });
  }
  return blocks;
}

/** The part of `block` that the linked `span` marks: the part with
 * exactly its lines and characters, or else the block itself. Undefined
 * when the span lies outside the block. */
export function linkedPart(
  block: Block,
  span: { start: number; end: number; chars?: [number, number] },
): Part | undefined {
  const [start, end] = block.lines;
  if (!block.fromSource || span.end < start || span.start > end) {
    return undefined;
  }
  const same = (part: Part) =>
    part.lines[0] === span.start &&
    part.lines[1] === span.end &&
    part.chars?.join("-") === span.chars?.join("-");
  return block.parts.find(same) ?? block.parts[0];
}

/** `node` with `className` added to each element of `marks`, as a copy of
 * the elements on the path to them. The rest of the tree is shared. */
export function withClasses(
  node: Element,
  marks: readonly (readonly [Element | undefined, string])[],
): Element {
  if (marks.every(([el]) => !el)) return node;
  const own = marks.filter(([el]) => el === node).map(([, cls]) => cls);
  const children = node.children.map((child) =>
    child.type === "element" ? withClasses(child, marks) : child,
  );
  const changed = children.some((child, i) => child !== node.children[i]);
  if (own.length === 0 && !changed) return node;
  const className = [
    [node.properties.className ?? []].flat().map(String),
    own,
  ].flat();
  return { ...node, properties: { ...node.properties, className }, children };
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
    if (b?.fromSource && b.lines[0] <= line) lo = mid + 1;
    else hi = mid;
  }
  const found = blocks[Math.max(lo - 1, 0)];
  return found?.fromSource ? found : undefined;
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
  /** The changed blocks that only a rebase changed, with none of the
   * author's edits. */
  drift: Set<Block>;
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
  const doc: RenderedDoc = {
    old: parse("old"),
    new: parse("new"),
    rows: [],
    drift: new Set(),
  };
  if (only) {
    doc.rows = doc[only].map((same) => ({ side: only, same }));
    return doc;
  }

  // Where each line sits in the diff, whether the diff adds or removes
  // it, whether the author did, and the line of context that it matches
  // on the other side.
  const pos = {
    old: new Map<number, number>(),
    new: new Map<number, number>(),
  };
  const edited = { old: new Set<number>(), new: new Set<number>() };
  const authored = { old: new Set<number>(), new: new Set<number>() };
  const match = {
    old: new Map<number, number>(),
    new: new Map<number, number>(),
  };
  for (const [i, line] of lines.entries()) {
    if (line.old !== undefined) pos.old.set(line.old, i);
    if (line.new !== undefined) pos.new.set(line.new, i);
    const mark = (side: Side, n: number | undefined) => {
      if (n === undefined) return;
      edited[side].add(n);
      if (!line.drift) authored[side].add(n);
    };
    if (line.kind === "del") mark("old", line.old);
    if (line.kind === "add") mark("new", line.new);
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
  /** The blocks that hold one of `lines`, on each side. */
  const blocksOf = (lines: Record<Side, Set<number>>) => ({
    old: new Set([...lines.old].map((n) => holder("old", n))),
    new: new Set([...lines.new].map((n) => holder("new", n))),
  });
  const editedBlocks = blocksOf(edited);
  const authoredBlocks = blocksOf(authored);
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
  /** Whether `block` is one of `blocks`, or its lines match one of them
   * on the other side. */
  const touches = (
    blocks: Record<Side, Set<Block | undefined>>,
    side: Side,
    block: Block,
  ) =>
    blocks[side].has(block) ||
    partners(side, block).some((b) => blocks[other(side)].has(b));
  const changed = (side: Side, block: Block) =>
    block.fromSource && touches(editedBlocks, side, block);

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
    if (!touches(authoredBlocks, side, block)) doc.drift.add(block);
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
