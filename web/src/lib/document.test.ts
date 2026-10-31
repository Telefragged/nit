import { describe, expect, it } from "vitest";
import type { RootContent } from "hast";
import type { LineAnchor } from "../api/types";
import { add, ctx as both, del, drift } from "../api/fixtures/builders";
import {
  blockAt,
  coveringPart,
  type DocRow,
  linkedPart,
  parseBlocks,
  renderDoc,
  shownRows,
  withClasses,
} from "./document";

const doc = [
  "# Plan", // 1
  "", // 2
  "First line of a paragraph", // 3
  "and its second line.", // 4
  "", // 5
  "1. one", // 6
  "2. two", // 7
  "", // 8
  "```", // 9
  "code", // 10
  "```", // 11
].join("\n");

describe("parseBlocks", () => {
  const blocks = parseBlocks(doc);

  it("splits a top-level list into its items", () => {
    expect(blocks.map((b) => b.lines)).toEqual([
      [1, 1],
      [3, 4],
      [6, 6],
      [7, 7],
      [9, 11],
    ]);
  });

  it("keeps the length of a block's last line", () => {
    expect(blocks[1]?.endChar).toBe(20);
  });

  it("keeps the number of an ordered item", () => {
    expect(blocks[3]?.node.tagName).toBe("ol");
    expect(blocks[3]?.node.properties.start).toBe(2);
  });

  it("ends an unclosed fence on its last line of text", () => {
    const [fence] = parseBlocks("```\ncode\n");
    expect(fence?.lines).toEqual([1, 2]);
  });

  it("marks the footnotes as no source lines", () => {
    const blocks = parseBlocks("Text[^1]\n\n[^1]: The note.");
    expect(blocks.map((b) => b.fromSource)).toEqual([true, false]);
  });

  it("tags the parts of a block with their source spans", () => {
    const [table] = parseBlocks("| A | Bee |\n|---|-----|\n| x | yz  |");
    const parts: unknown[] = [];
    const walk = (node: RootContent) => {
      if (node.type !== "element") return;
      if (node.properties.dataPart !== undefined) {
        const { tagName, properties: p } = node;
        parts.push([tagName, p.dataStart, p.dataEnd, p.dataChars]);
      }
      node.children.forEach(walk);
    };
    if (table) walk(table.node);
    expect(parts).toEqual([
      ["tr", 1, 1, undefined],
      ["th", 1, 1, "2-3"],
      ["th", 1, 1, "6-9"],
      ["tr", 3, 3, undefined],
      ["td", 3, 3, "2-3"],
      ["td", 3, 3, "6-8"],
    ]);
  });

  it("splits a fence's code into one tagged span per line", () => {
    const [, fence] = parseBlocks("text\n\n```js\nlet a;\nlet b;\n```");
    const code = fence?.node.children[0];
    const lines =
      code?.type === "element"
        ? code.children.flatMap((c) =>
            c.type === "element"
              ? [[c.properties.dataStart, c.properties.dataLang]]
              : [],
          )
        : [];
    expect(lines).toEqual([
      [4, "js"],
      [5, "js"],
    ]);
  });

  it("drops a javascript: link", () => {
    const [p] = parseBlocks("[x](javascript:alert(1))");
    const link = p?.node.children[0];
    expect(link?.type === "element" && link.properties.href).toBe("");
  });
});

describe("linkedPart", () => {
  const [table] = parseBlocks("| A | B |\n|---|---|\n| x | y |");
  if (!table) throw new Error("no table");
  const linked = (start: number, end: number, chars?: [number, number]) =>
    linkedPart(table, { start, end, ...(chars && { chars }) })?.node?.tagName;

  it("marks the part with exactly the linked lines and characters", () => {
    expect(linked(3, 3, [6, 7])).toBe("td");
    expect(linked(3, 3)).toBe("tr");
  });

  it("marks the block itself for another span inside it, and none outside", () => {
    expect(linkedPart(table, { start: 2, end: 2 })).toBe(table.parts[0]);
    expect(linkedPart(table, { start: 5, end: 5 })).toBeUndefined();
  });
});

describe("coveringPart", () => {
  const [table] = parseBlocks("| A | B |\n|---|---|\n| x | y |");
  if (!table) throw new Error("no table");
  const covering = (at: LineAnchor) => coveringPart(table, at)?.node?.tagName;

  it("finds the innermost part that covers an anchor", () => {
    const sel = { start_line: 3, start_char: 6, end_line: 3, end_char: 7 };
    expect(covering({ selection: sel })).toBe("td");
    expect(covering({ whole: 3 })).toBe("tr");
  });

  it("falls back to the block for an anchor that no part covers", () => {
    expect(coveringPart(table, { whole: 2 })).toBe(table.parts[0]);
  });
});

describe("withClasses", () => {
  it("adds a class to a marked element and copies only its ancestors", () => {
    const [list] = parseBlocks("- a\n- b");
    const item = list?.parts[1]?.node;
    if (!list || !item) throw new Error("no item");
    const marked = withClasses(list.node, [[item, "linked"]]);
    expect(marked).not.toBe(list.node);
    const copy = marked.children.find((c) => c.type === "element");
    expect(copy?.type === "element" && copy.properties.className).toEqual([
      "linked",
    ]);
    expect(withClasses(list.node, [[undefined, "linked"]])).toBe(list.node);
  });
});

describe("blockAt", () => {
  const blocks = parseBlocks(doc);

  it("places a line under the block that holds it", () => {
    expect(blockAt(blocks, 4)?.lines).toEqual([3, 4]);
  });

  it("places a blank line under the block above it", () => {
    expect(blockAt(blocks, 5)?.lines).toEqual([3, 4]);
  });

  it("places a line past the end under the last block", () => {
    expect(blockAt(blocks, 40)?.lines).toEqual([9, 11]);
  });
});

describe("renderDoc", () => {
  /** Each row as the first source line of its blocks: `=n` for a block
   * that stays, `-n` and `+n` for a removed and an added one. */
  const shape = (lines: Parameters<typeof renderDoc>[0]) =>
    renderDoc(lines).rows.map((row) =>
      "same" in row
        ? `=${row.same.lines[0]}`
        : [
            ...row.removed.map((b) => `-${b.lines[0]}`),
            ...row.added.map((b) => `+${b.lines[0]}`),
          ].join(" "),
    );

  it("puts a removed block above the block that replaces it", () => {
    const rows = shape([
      both(1, 1, "# Title"),
      both(2, 2, ""),
      del(3, "old"),
      add(3, "new"),
      both(4, 4, ""),
      both(5, 5, "end"),
    ]);
    expect(rows).toEqual(["=1", "-3 +3", "=5"]);
  });

  it("changes both sides of a block that loses one of its lines", () => {
    const rows = shape([both(1, 1, "one"), del(2, "two"), both(3, 2, "three")]);
    expect(rows).toEqual(["-1 +1"]);
  });

  it("matches a block that stays with its old side", () => {
    const doc = renderDoc([del(1, "gone"), both(2, 1, ""), both(3, 2, "kept")]);
    const kept = doc.rows.at(-1);
    expect(kept && "same" in kept && kept.old?.lines).toEqual([3, 3]);
  });

  it("marks a block that only a rebase changed as drift", () => {
    const doc = renderDoc([
      drift(del(1, "base")),
      drift(add(1, "moved base")),
      both(2, 2, ""),
      del(3, "mine"),
      add(3, "my edit"),
    ]);
    const changed = doc.rows.flatMap((row) =>
      "same" in row ? [] : [...row.removed, ...row.added],
    );
    expect(changed.map((b) => [b.lines[0], doc.drift.has(b)])).toEqual([
      [1, true],
      [3, false],
      [1, true],
      [3, false],
    ]);
  });

  it("keeps the author's colors for a block with drift and an edit", () => {
    const doc = renderDoc([
      drift(del(1, "base")),
      drift(add(1, "moved base")),
      del(2, "mine"),
      add(2, "my edit"),
    ]);
    expect(doc.drift.size).toBe(0);
  });

  it("renders the one side of a file with no changes", () => {
    const doc = renderDoc([add(1, "# New"), add(2, "")], "new");
    expect(doc.rows).toHaveLength(1);
    expect(doc.old).toEqual([]);
  });
});

describe("shownRows", () => {
  const [block] = parseBlocks("text");
  if (!block) throw new Error("no block");
  const same: DocRow = { side: "new", same: block };
  const change: DocRow = { removed: [block], added: [] };
  /** Each shown row as its index, each hidden run as `from-to`. */
  const shape = (
    rows: DocRow[],
    keep: (row: DocRow, index: number) => boolean = () => false,
  ) =>
    shownRows(rows, keep).map((s) =>
      "from" in s ? `${s.from}-${s.to}` : s.index,
    );

  it("hides the rows past the context of a change", () => {
    const rows = [same, same, same, change, same, same, same];
    expect(shape(rows)).toEqual(["0-1", 2, 3, 4, "5-6"]);
  });

  it("shows a row that keep holds for", () => {
    const rows = [same, same, same, change, same];
    expect(shape(rows, (_, i) => i === 0)).toEqual([0, "1-1", 2, 3, 4]);
  });

  it("shows every row of a file with no change", () => {
    expect(shape([same, same, same])).toEqual([0, 1, 2]);
  });
});
