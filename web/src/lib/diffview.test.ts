import { describe, expect, it } from "vitest";
import type { DiffFile, Line } from "../api/types";
import { COMMIT_MSG_PATH } from "../api/types";
import {
  gapLines,
  intralineMarks,
  pairLines,
  rangeSliceOnLine,
  skippedBefore,
  treeOrder,
} from "./diffview";

const ctx = (old: number, nw: number, text = "ctx"): Line => ({
  kind: "context",
  old,
  new: nw,
  text,
});
const add = (nw: number, text = "add"): Line => ({
  kind: "add",
  new: nw,
  text,
});
const del = (old: number, text = "del"): Line => ({ kind: "del", old, text });

describe("pairLines", () => {
  it("mirrors context lines onto both sides", () => {
    expect(pairLines([ctx(1, 1), ctx(2, 2)])).toEqual([
      { left: ctx(1, 1), right: ctx(1, 1) },
      { left: ctx(2, 2), right: ctx(2, 2) },
    ]);
  });

  it("aligns a del run index-wise with the add run that follows", () => {
    const lines = [del(5), del(6), add(5), ctx(7, 6)];
    expect(pairLines(lines)).toEqual([
      { left: del(5), right: add(5) },
      { left: del(6), right: null },
      { left: ctx(7, 6), right: ctx(7, 6) },
    ]);
  });

  it("pads a longer add run with empty left cells", () => {
    const lines = [del(3), add(3), add(4)];
    expect(pairLines(lines)).toEqual([
      { left: del(3), right: add(3) },
      { left: null, right: add(4) },
    ]);
  });
});

describe("intralineMarks", () => {
  it("marks a replacement block whose sides differ in length", () => {
    const gone = del(1, "out = compute(x, y) + 1;");
    const first = add(1, "temp = compute(x, y);");
    const second = add(2, "out = temp + 1;");
    const marks = intralineMarks([gone, first, second]);
    expect(marks.get(gone)).toEqual([[0, 3]]);
    expect(marks.get(first)).toEqual([
      [0, 4],
      [20, 21],
    ]);
    expect(marks.get(second)).toEqual([[0, 10]]);
  });

  it("marks every changed run of a line", () => {
    const d = del(1, "fn f(a: u8, b: u8)");
    const a = add(1, "fn f(a: u32, b: u32)");
    expect(intralineMarks([d, a]).get(a)).toEqual([
      [9, 11],
      [17, 19],
    ]);
  });

  it("leaves a run with no counterpart unmarked", () => {
    const lines = [del(1, "gone"), del(2, "also gone"), ctx(3, 1)];
    expect(intralineMarks(lines).size).toBe(0);
  });
});

describe("skippedBefore", () => {
  const hunk = (
    oldStart: number,
    oldLines: number,
    newStart: number,
    newLines: number,
  ) => ({
    old_start: oldStart,
    old_lines: oldLines,
    new_start: newStart,
    new_lines: newLines,
    header: "",
    lines: [],
  });

  it("counts the lines before the first hunk", () => {
    expect(skippedBefore(undefined, hunk(10, 3, 12, 3))).toBe(11);
  });

  it("is zero when the file starts at the first hunk", () => {
    expect(skippedBefore(undefined, hunk(1, 3, 1, 3))).toBe(0);
  });

  it("takes the larger of the old/new gaps between hunks", () => {
    expect(skippedBefore(hunk(1, 3, 1, 5), hunk(10, 2, 8, 2))).toBe(6);
  });

  it("is zero for adjacent hunks", () => {
    expect(skippedBefore(hunk(1, 3, 1, 3), hunk(4, 2, 4, 2))).toBe(0);
  });

  it("counts up to the line a side carrying nothing sits after", () => {
    expect(skippedBefore(hunk(1, 3, 1, 3), hunk(10, 0, 4, 2))).toBe(7);
  });
});

describe("gapLines", () => {
  const hunk = (oldStart: number, newStart: number) => ({
    old_start: oldStart,
    old_lines: 1,
    new_start: newStart,
    new_lines: 1,
    header: "",
    lines: [],
  });

  // A file whose full diff has a drift del between two changed lines.
  const full: Line[] = [
    ctx(1, 1),
    ctx(2, 2),
    ctx(3, 3),
    del(4, "dropped by the base"), // drift: old-only, in the gap
    ctx(5, 4),
    ctx(6, 5),
  ];

  it("returns the run between two hunks, del lines included", () => {
    const out = gapLines(full, hunk(3, 3), hunk(6, 5));
    expect(out).toEqual([del(4, "dropped by the base"), ctx(5, 4)]);
  });

  it("returns the run above the first hunk", () => {
    expect(gapLines(full, undefined, hunk(3, 3))).toEqual([
      ctx(1, 1),
      ctx(2, 2),
    ]);
  });

  // An outline pairs the two `}` its collapse left adjacent, where the
  // whole file pairs each `}` with its own body — so the body sits in the
  // gap on the old side and below the last hunk on the new one.
  const outlined: Line[] = [
    ctx(1, 1, "fn a() {"),
    add(2, "}"),
    add(3, ""),
    add(4, "fn b() {"),
    ctx(2, 5, "    body"),
    ctx(3, 6, "    more"),
    ctx(4, 7, "}"),
  ];
  const signature = { ...hunk(1, 1), old_lines: 1, new_lines: 1 };
  const braces = { ...hunk(4, 2), old_lines: 1, new_lines: 3 };

  it("reveals a line on whichever side of it the gap holds", () => {
    expect(gapLines(outlined, signature, braces)).toEqual([
      del(2, "    body"),
      del(3, "    more"),
    ]);
    expect(gapLines(outlined, braces, undefined)).toEqual([
      add(5, "    body"),
      add(6, "    more"),
      add(7, "}"),
    ]);
  });
});

describe("rangeSliceOnLine", () => {
  const range = { start_line: 12, start_char: 4, end_line: 14, end_char: 7 };

  it("misses lines outside the range", () => {
    expect(rangeSliceOnLine(range, 11, 20)).toBeNull();
    expect(rangeSliceOnLine(range, 15, 20)).toBeNull();
  });

  it("starts at start_char on the first line and runs to its end", () => {
    expect(rangeSliceOnLine(range, 12, 20)).toEqual([4, 20]);
  });

  it("covers interior lines whole", () => {
    expect(rangeSliceOnLine(range, 13, 9)).toEqual([0, 9]);
  });

  it("ends at end_char on the last line", () => {
    expect(rangeSliceOnLine(range, 14, 20)).toEqual([0, 7]);
  });

  it("clamps offsets to the text and drops empty windows", () => {
    expect(rangeSliceOnLine(range, 14, 5)).toEqual([0, 5]);
    expect(rangeSliceOnLine(range, 12, 3)).toBeNull(); // start past the text
    expect(rangeSliceOnLine(range, 13, 0)).toBeNull(); // empty interior line
  });

  it("handles a single-line range", () => {
    const one = { start_line: 5, start_char: 2, end_line: 5, end_char: 6 };
    expect(rangeSliceOnLine(one, 5, 10)).toEqual([2, 6]);
    expect(rangeSliceOnLine(one, 4, 10)).toBeNull();
  });
});

describe("treeOrder", () => {
  const file = (path: string): DiffFile => ({
    path,
    status: "modified",
    binary: false,
    additions: 1,
    deletions: 1,
    new_total: 0,
    hunks: [],
  });
  const paths = (files: DiffFile[]) => treeOrder(files).map((f) => f.path);

  it("puts directories before files at every level", () => {
    // Git's own order is the plain path sort this shuffles: flake.nix and
    // web/package.json both sort ahead of their sibling directories there.
    expect(
      paths([
        file("docs/dev.md"),
        file("flake.nix"),
        file("web/package.json"),
        file("web/src/App.tsx"),
        file("docs/api.md"),
      ]),
    ).toEqual([
      "docs/api.md",
      "docs/dev.md",
      "web/src/App.tsx",
      "web/package.json",
      "flake.nix",
    ]);
  });

  it("counts digit runs, so a9 precedes a10", () => {
    expect(paths([file("m/a10.sql"), file("m/a9.sql")])).toEqual([
      "m/a9.sql",
      "m/a10.sql",
    ]);
  });

  it("leads with the commit message, which is not a file", () => {
    expect(paths([file("a.rs"), file(COMMIT_MSG_PATH)])).toEqual([
      COMMIT_MSG_PATH,
      "a.rs",
    ]);
  });

  it("leaves the caller's array alone", () => {
    const input = [file("b.rs"), file("a.rs")];
    treeOrder(input);
    expect(input.map((f) => f.path)).toEqual(["b.rs", "a.rs"]);
  });
});
