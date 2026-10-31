import { describe, expect, it } from "vitest";
import { linkParams, parseLink, pressLink } from "./diffLink";

const parse = (query: string) => parseLink(new URLSearchParams(query));

describe("parseLink", () => {
  it("reads a file, a line, a span, and the old side", () => {
    expect(parse("revision=1")).toBeNull();
    expect(parse("file=a.rs")).toEqual({ file: "a.rs", lines: null });
    expect(parse("file=a.rs&line=7")).toEqual({
      file: "a.rs",
      lines: { side: "new", start: 7, end: 7 },
    });
    expect(parse("file=a.rs&line=9-4&side=old")).toEqual({
      file: "a.rs",
      lines: { side: "old", start: 4, end: 9 },
    });
  });

  it("reads a thread before a file", () => {
    expect(parse("thread=42&file=a.rs")).toEqual({ thread: 42 });
    expect(parse("thread=x&file=a.rs")).toEqual({ file: "a.rs", lines: null });
  });

  it("links the whole file when the line is malformed", () => {
    expect(parse("file=a.rs&line=x")).toEqual({ file: "a.rs", lines: null });
  });

  it("is the inverse of linkParams", () => {
    for (const query of [
      "file=a.rs",
      "file=a.rs&line=7",
      "file=a&line=4-9",
      "thread=42",
    ]) {
      const link = parse(query);
      if (link === null) throw new Error(query);
      const params = new URLSearchParams();
      for (const [key, value] of Object.entries(linkParams(link)))
        if (value !== null) params.set(key, value);
      expect(parseLink(params)).toEqual(link);
    }
  });
});

describe("pressLink", () => {
  const span = pressLink(null, "a.rs", "new", [10, 10], false).span;

  it("links the clicked line", () => {
    expect(span).toEqual({
      file: "a.rs",
      lines: { side: "new", start: 10, end: 10 },
    });
  });

  it("stretches the span on an extending click, in either direction", () => {
    const down = pressLink(span, "a.rs", "new", [14, 14], true).span;
    expect(down.lines).toEqual({ side: "new", start: 10, end: 14 });
    expect(pressLink(down, "a.rs", "new", [6, 6], true).span.lines).toEqual({
      side: "new",
      start: 6,
      end: 14,
    });
  });

  it("starts over on another side or file", () => {
    expect(pressLink(span, "a.rs", "old", [14, 14], true).span.lines).toEqual({
      side: "old",
      start: 14,
      end: 14,
    });
    expect(pressLink(span, "b.rs", "new", [14, 14], true).span.lines).toEqual({
      side: "new",
      start: 14,
      end: 14,
    });
  });
});
