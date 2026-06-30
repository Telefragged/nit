import { describe, expect, it } from "vitest";
import type { DiffFile } from "../api/types";
import { COMMIT_MSG_PATH } from "../api/types";
import {
  allExpanded,
  collapseAll,
  defaultExpanded,
  expand,
  expandAll,
  toggle,
} from "./collapse";

const file = (path: string): DiffFile => ({
  path,
  status: "modified",
  binary: false,
  additions: 1,
  deletions: 1,
  new_total: 0,
  hunks: [],
});

const files = [file(COMMIT_MSG_PATH), file("src/a.rs"), file("src/b.rs")];

describe("defaultExpanded", () => {
  it("starts with only the commit message expanded", () => {
    const cur = defaultExpanded();
    expect(cur.has(COMMIT_MSG_PATH)).toBe(true);
    expect(cur.size).toBe(1);
  });
});

describe("expand", () => {
  it("adds a collapsed file", () => {
    const cur = expand(defaultExpanded(), "src/a.rs");
    expect(cur.has("src/a.rs")).toBe(true);
    expect(cur.has(COMMIT_MSG_PATH)).toBe(true);
  });

  it("returns the same reference when already expanded (no-op render)", () => {
    const cur = expand(defaultExpanded(), "src/a.rs");
    expect(expand(cur, "src/a.rs")).toBe(cur);
  });

  it("does not mutate its input", () => {
    const cur = defaultExpanded();
    expand(cur, "src/a.rs");
    expect(cur.has("src/a.rs")).toBe(false);
  });
});

describe("toggle", () => {
  it("expands a collapsed file and collapses an expanded one", () => {
    const once = toggle(defaultExpanded(), "src/a.rs");
    expect(once.has("src/a.rs")).toBe(true);
    const twice = toggle(once, "src/a.rs");
    expect(twice.has("src/a.rs")).toBe(false);
    // The rest of the set is untouched either way.
    expect(twice.has(COMMIT_MSG_PATH)).toBe(true);
  });
});

describe("expandAll / collapseAll / allExpanded", () => {
  it("expandAll covers every file of the diff", () => {
    const cur = expandAll(files);
    expect(allExpanded(cur, files)).toBe(true);
    expect(cur.size).toBe(files.length);
  });

  it("allExpanded is false while any file is collapsed", () => {
    expect(allExpanded(defaultExpanded(), files)).toBe(false);
    expect(allExpanded(expand(defaultExpanded(), "src/a.rs"), files)).toBe(
      false,
    );
  });

  it("collapseAll collapses everything, commit message included", () => {
    const cur = collapseAll();
    expect(cur.size).toBe(0);
    expect(allExpanded(cur, files)).toBe(false);
  });

  it("an empty diff is never 'all expanded'", () => {
    expect(allExpanded(collapseAll(), [])).toBe(false);
  });
});
