import { describe, expect, it } from "vitest";
import { parseBlocks } from "./document";
import { blockSource, withBlock, withoutBlock } from "./documentText";

const TEXT = "# Title\n\nFirst 😀 paragraph.\n\n- one\n- two\n\nLast.\n";

describe("blockSource", () => {
  it("returns the lines of each block", () => {
    expect(
      parseBlocks(TEXT).map((b) => blockSource(TEXT.split("\n"), b.lines)),
    ).toEqual(["# Title", "First 😀 paragraph.", "- one", "- two", "Last."]);
  });
});

describe("withBlock", () => {
  it("replaces the lines of the block, and no other line", () => {
    expect(withBlock(TEXT, [3, 3], "A.\n\nB.")).toBe(
      "# Title\n\nA.\n\nB.\n\n- one\n- two\n\nLast.\n",
    );
  });

  it("keeps the indent of the first line", () => {
    expect(withBlock("a\n\n    code\n", [3, 3], "other")).toBe(
      "a\n\n    other\n",
    );
  });
});

describe("withoutBlock", () => {
  it("removes the block and the blank line below it", () => {
    expect(withoutBlock(TEXT, [3, 3])).toBe(
      "# Title\n\n- one\n- two\n\nLast.\n",
    );
  });

  it("removes an item of a list, and no blank line beside the list", () => {
    expect(withoutBlock(TEXT, [5, 5])).toBe(
      "# Title\n\nFirst 😀 paragraph.\n\n- two\n\nLast.\n",
    );
    expect(withoutBlock(TEXT, [6, 6])).toBe(
      "# Title\n\nFirst 😀 paragraph.\n\n- one\n\nLast.\n",
    );
  });

  it("removes the last block", () => {
    expect(withoutBlock("a\n\nb", [3, 3])).toBe("a\n");
    expect(withoutBlock("a\n\nb\n", [3, 3])).toBe("a\n");
  });

  it("removes the first block and the blank line below it", () => {
    expect(withoutBlock("a\n\nb\n", [1, 1])).toBe("b\n");
  });

  it("removes the only block", () => {
    expect(withoutBlock("a\n", [1, 1])).toBe("");
  });
});
