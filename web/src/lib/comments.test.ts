import { describe, expect, it } from "vitest";
import type { CommentAnchor } from "./comments";
import { commentPlacement, draftAnchor } from "./comments";

const anchor = (revision: number, side: "old" | "new", line: number | null) =>
  ({ revision, side, line }) satisfies CommentAnchor;

describe("commentPlacement", () => {
  // Base diff (FROM = base): r2's new side is the right column, its old
  // side (the parent) the left column.
  describe("base → rN", () => {
    it("puts the TO new side on the right", () => {
      expect(commentPlacement(anchor(2, "new", 14), 2, undefined)).toEqual({
        side: "new",
        line: 14,
      });
    });
    it("puts the TO old side (the parent) on the left", () => {
      expect(commentPlacement(anchor(2, "old", 9), 2, undefined)).toEqual({
        side: "old",
        line: 9,
      });
    });
    it("hides a comment from another revision", () => {
      expect(commentPlacement(anchor(1, "new", 5), 2, undefined)).toBeNull();
    });
  });

  // Interdiff rM → rN: the left column is rM's own content, so a comment
  // made on rM (its new side) renders there.
  describe("rM → rN", () => {
    it("puts the TO new side on the right", () => {
      expect(commentPlacement(anchor(3, "new", 20), 3, 1)).toEqual({
        side: "new",
        line: 20,
      });
    });
    it("puts the FROM revision's new side on the left", () => {
      expect(commentPlacement(anchor(1, "new", 7), 3, 1)).toEqual({
        side: "old",
        line: 7,
      });
    });
    it("hides a base-side (old) comment — there is no parent column", () => {
      expect(commentPlacement(anchor(3, "old", 4), 3, 1)).toBeNull();
      expect(commentPlacement(anchor(1, "old", 4), 3, 1)).toBeNull();
    });
    it("hides a comment on a revision that is neither FROM nor TO", () => {
      expect(commentPlacement(anchor(2, "new", 9), 3, 1)).toBeNull();
    });
  });

  it("never places a line-less (file-level) comment", () => {
    expect(commentPlacement(anchor(2, "new", null), 2, undefined)).toBeNull();
  });
});

describe("draftAnchor", () => {
  it("anchors a new-column draft to the selected revision", () => {
    expect(draftAnchor("new", 3, undefined)).toEqual({
      revision: 3,
      side: "new",
    });
    expect(draftAnchor("new", 3, 1)).toEqual({ revision: 3, side: "new" });
  });

  it("anchors a base old-column draft to the selected revision's parent", () => {
    expect(draftAnchor("old", 3, undefined)).toEqual({
      revision: 3,
      side: "old",
    });
  });

  it("anchors an interdiff old-column draft to the FROM revision's content", () => {
    expect(draftAnchor("old", 3, 1)).toEqual({ revision: 1, side: "new" });
  });

  it("is the inverse of commentPlacement", () => {
    // A draft on a column round-trips: store it, then place it back into
    // the same range and it lands on the column it was drawn on.
    for (const [selected, against] of [
      [3, undefined],
      [3, 1],
    ] as const) {
      for (const column of ["old", "new"] as const) {
        const stored = draftAnchor(column, selected, against);
        const placed = commentPlacement(
          { ...stored, line: 12 },
          selected,
          against,
        );
        expect(placed).toEqual({ side: column, line: 12 });
      }
    }
  });
});
