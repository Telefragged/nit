import { describe, expect, it } from "vitest";
import { buildTree, fileOrder, type TreeNode } from "./fileTree";

/** The rows of `tree` as indented names, the way the rail lists them. */
const rows = (tree: TreeNode[], depth = 0): string[] =>
  tree.flatMap((n) => [
    `${"  ".repeat(depth)}${n.name}${n.kind === "dir" ? "/" : ""}`,
    ...(n.kind === "dir" ? rows(n.children, depth + 1) : []),
  ]);

describe("buildTree", () => {
  it("merges a directory that holds only one directory into it", () => {
    expect(
      rows(buildTree(["web/src/components/diff/FileRail.tsx", "web/a.ts"])),
    ).toEqual(["web/", "  src/components/diff/", "    FileRail.tsx", "  a.ts"]);
  });

  it("keeps a directory that holds a file beside its one directory", () => {
    expect(rows(buildTree(["a/b/c.rs", "a/d.rs"]))).toEqual([
      "a/",
      "  b/",
      "    c.rs",
      "  d.rs",
    ]);
  });

  it("keeps the index of each path, in the order it lists them", () => {
    const paths = ["b.rs", "a/z.rs", "a.rs"];
    expect(fileOrder(buildTree(paths)).map((i) => paths[i])).toEqual([
      "a/z.rs",
      "a.rs",
      "b.rs",
    ]);
  });
});
