import { describe, expect, it } from "vitest";
import {
  alignText,
  formatSourceMap,
  parseSourceMap,
  renderedSlice,
  sourceAt,
} from "./sourceMap";

const places = (value: string, source: string, line = 1, char = 0) => {
  const map = alignText(value, source, { line, char });
  return Array.from({ length: value.length }, (_, i) => {
    const at = sourceAt(map, i);
    return `${at.line}:${at.char}`;
  });
};

describe("alignText", () => {
  it("maps text that renders as written to one run", () => {
    expect(alignText("plain", "plain", { line: 3, char: 4 })).toEqual([
      { at: 0, line: 3, char: 4 },
    ]);
  });

  it("skips a backslash escape", () => {
    expect(places("a*b", "a\\*b")).toEqual(["1:0", "1:2", "1:3"]);
  });

  it("maps a character reference to its start", () => {
    expect(places("a©b", "a&copy;b")).toEqual(["1:0", "1:1", "1:7"]);
    expect(places("a&b", "a&amp;b")).toEqual(["1:0", "1:1", "1:6"]);
  });

  it("skips the block quote marker of the next line", () => {
    expect(places("a\nb", "a\n> b", 2, 2)).toEqual(["2:2", "2:3", "3:2"]);
  });

  it("maps a character missing from the source to the search start", () => {
    expect(places("ab", "b")).toEqual(["1:0", "1:0"]);
  });
});

describe("renderedSlice", () => {
  const map = alignText("one\ntwo", "one\n  two", { line: 1, char: 0 });

  it("finds the rendered characters inside a range", () => {
    const range = { start_line: 1, start_char: 1, end_line: 2, end_char: 3 };
    expect(renderedSlice(map, 7, range)).toEqual([1, 5]);
  });

  it("finds none for a range outside the text", () => {
    const range = { start_line: 3, start_char: 0, end_line: 3, end_char: 1 };
    expect(renderedSlice(map, 7, range)).toBeNull();
  });
});

describe("parseSourceMap", () => {
  it("parses the map that formatSourceMap writes", () => {
    const map = alignText("a*b", "a\\*b", { line: 4, char: 1 });
    expect(parseSourceMap(formatSourceMap(map))).toEqual(map);
  });
});
