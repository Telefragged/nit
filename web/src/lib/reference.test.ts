import { describe, expect, it } from "vitest";
import { add, ctx, del } from "../api/fixtures/builders";
import type { Line } from "../api/types";
import type { ReviewFile } from "./attachments";
import { parseBlocks } from "./document";
import {
  changeHref,
  rangeFile,
  referencesByLine,
  uploadMarkdown,
} from "./reference";

const WHOLE: Line[] = [
  ctx(1, 1, "a"),
  ctx(2, 2, "b"),
  del(3, "c"),
  del(4, "d"),
  add(3, "C"),
  add(4, "D"),
  add(5, "E"),
  ctx(5, 6, "f"),
  ctx(6, 7, "g"),
];

const FILE: ReviewFile = {
  path: "src/a.rs",
  status: "modified",
  binary: false,
  additions: 3,
  deletions: 2,
  old_total: 6,
  new_total: 7,
  hunks: [],
};

const hunkOf = (side: "old" | "new", start: number, end: number) =>
  rangeFile(FILE, WHOLE, { side, start, end })?.hunks[0];

describe("rangeFile", () => {
  it("cuts the file to the lines of a new-side range", () => {
    expect(hunkOf("new", 4, 6)).toEqual({
      old_start: 5,
      old_lines: 1,
      new_start: 4,
      new_lines: 3,
      header: "",
      lines: [add(4, "D"), add(5, "E"), ctx(5, 6, "f")],
    });
  });

  it("keeps the lines of the other side between the ends", () => {
    expect(hunkOf("new", 2, 3)?.lines).toEqual([
      ctx(2, 2, "b"),
      del(3, "c"),
      del(4, "d"),
      add(3, "C"),
    ]);
  });

  it("declares the line before a hunk that has no line on a side", () => {
    expect(hunkOf("new", 3, 5)).toMatchObject({
      old_start: 4,
      old_lines: 0,
      new_start: 3,
      new_lines: 3,
    });
  });

  it("cuts a range of the old side", () => {
    expect(hunkOf("old", 3, 4)?.lines).toEqual([del(3, "c"), del(4, "d")]);
  });

  it("returns null when the file lacks an end of the range", () => {
    expect(
      rangeFile(FILE, WHOLE, { side: "new", start: 6, end: 9 }),
    ).toBeNull();
  });
});

describe("changeHref", () => {
  it("writes the diff range and the linked lines", () => {
    expect(
      changeHref(11, 2, "r1", {
        file: "src/a.rs",
        lines: { side: "old", start: 3, end: 4 },
      }),
    ).toBe(
      "/changes/11?revision=2&against=r1&file=src%2Fa.rs&line=3-4&side=old",
    );
  });

  it("writes no base", () => {
    expect(changeHref(11, 2, "r2^", null)).toBe("/changes/11?revision=2");
  });
});

const split = (text: string) => {
  const references = referencesByLine(text);
  return parseBlocks(text).map(
    (block) => references.get(block.lines[0] - 1) ?? null,
  );
};

describe("referencesByLine", () => {
  const FILE = {
    type: "file",
    change: 1,
    revision: 0,
    against: null,
    file: "a.rs",
    lines: null,
  };

  it("makes a reference of a link that is a whole paragraph", () => {
    expect(
      split(
        "Narration.\n\n[change 1](/changes/1)\n\n[a.rs](/changes/1?revision=2&against=r1&file=src%2Fa.rs&line=3-4&side=old)\n",
      ),
    ).toEqual([
      null,
      { type: "change", change: 1 },
      {
        type: "file",
        change: 1,
        revision: 2,
        against: 1,
        file: "src/a.rs",
        lines: { side: "old", start: 3, end: 4 },
      },
    ]);
  });

  it("makes a file reference of a link to an attachment", () => {
    expect(
      split("[plan](/changes/1?revision=0&file=%2Fattachments%2Fplan.md)"),
    ).toEqual([{ ...FILE, file: "/attachments/plan.md" }]);
  });

  it("makes a reference of an address on a line of its own", () => {
    expect(split("http://host/changes/1?revision=0&file=a.rs")).toEqual([FILE]);
  });

  it("makes an upload reference of an image and of a link", () => {
    const hash = "a".repeat(64);
    const upload = (name: string) => ({ type: "upload", hash, name });
    expect(
      split(
        `![sketch](/api/attachments/${hash}/sketch.png)\n\n[log](/api/attachments/${hash}/nit%20log.txt)`,
      ),
    ).toEqual([upload("sketch.png"), upload("nit log.txt")]);
  });

  it("keeps a link inside a sentence as narration", () => {
    expect(split("See [change 1](/changes/1) first.")).toEqual([null]);
    expect(split("[change 1](/changes/1) and [change 2](/changes/2)")).toEqual([
      null,
    ]);
  });

  it("keeps a link in an item of a list or in a quote as narration", () => {
    expect(
      split("- [change 1](/changes/1)\n\n> [change 1](/changes/1)"),
    ).toEqual([null, null]);
  });

  it("keeps a link to another address as narration", () => {
    expect(split("[docs](https://example.com/changes)")).toEqual([null]);
    expect(split("[chain](/chains/7)")).toEqual([null]);
  });
});

describe("uploadMarkdown", () => {
  const hash = "a".repeat(64);

  it("prints an image for an image, and else a link", () => {
    expect(uploadMarkdown(hash, "a b.png", true)).toBe(
      `![a b.png](/api/attachments/${hash}/a%20b.png)`,
    );
    expect(uploadMarkdown(hash, "log.txt", false)).toBe(
      `[log.txt](/api/attachments/${hash}/log.txt)`,
    );
  });

  it("prints a reference that the page parses for a name with brackets", () => {
    expect(split(uploadMarkdown(hash, "a[1].txt", false))).toEqual([
      { type: "upload", hash, name: "a[1].txt" },
    ]);
  });
});
