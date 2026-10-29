import { describe, expect, it } from "vitest";
import { blockAt, parseBlocks } from "./document";

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

  it("anchors a block to its whole lines", () => {
    expect(blocks[0]?.at).toEqual({ whole: 1 });
    expect(blocks[1]?.at).toEqual({
      selection: { start_line: 3, start_char: 0, end_line: 4, end_char: 20 },
    });
  });

  it("keeps the number of an ordered item", () => {
    expect(blocks[3]?.node.tagName).toBe("ol");
    expect(blocks[3]?.node.properties.start).toBe(2);
  });

  it("ends an unclosed fence on its last line of text", () => {
    const [fence] = parseBlocks("```\ncode\n");
    expect(fence?.lines).toEqual([1, 2]);
  });

  it("gives the footnotes no anchor", () => {
    const blocks = parseBlocks("Text[^1]\n\n[^1]: The note.");
    expect(blocks.map((b) => b.at)).toEqual([{ whole: 1 }, null]);
  });

  it("drops a javascript: link", () => {
    const [p] = parseBlocks("[x](javascript:alert(1))");
    const link = p?.node.children[0];
    expect(link?.type === "element" && link.properties.href).toBe("");
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
