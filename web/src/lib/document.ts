// Splits a Markdown file into the blocks that the review page renders and
// comments on one at a time.

import type { Element, Root, RootContent } from "hast";
import { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import remarkRehype from "remark-rehype";
import { unified } from "unified";
import type { LineAnchor } from "../api/types";

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
