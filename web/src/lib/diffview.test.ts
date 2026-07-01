import { describe, expect, it } from "vitest";
import type { Line } from "../api/types";
import {
  gapLines,
  intralineDiff,
  pairLines,
  rangeSliceOnLine,
  skippedBefore,
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

describe("intralineDiff", () => {
  it("marks only the differing middle of a similar pair", () => {
    expect(intralineDiff("let x = old_value;", "let x = new_value;")).toEqual([
      [8, 11],
      [8, 11],
    ]);
  });

  it("returns null for identical lines", () => {
    expect(intralineDiff("same", "same")).toBeNull();
  });

  it("returns null for a mostly rewritten line (similarity gate)", () => {
    expect(intralineDiff("abcdefghij", "a000000000")).toBeNull();
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
