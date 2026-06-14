import { describe, expect, it } from "vitest";
import type { Comment } from "../api/types";
import type { CommentAnchor } from "./comments";
import {
  commentCountLabel,
  commentPlacement,
  draftAnchor,
  pendingResolved,
  pendingUnresolvedCount,
  threadCountByRevision,
} from "./comments";

const anchor = (revision: number, side: "old" | "new", line: number | null) =>
  ({ revision, side, line }) satisfies CommentAnchor;

/** Minimal comment for the counting tests — only revision/parent_id matter. */
const comment = (
  id: number,
  revision: number,
  parent_id: number | null = null,
  state: "draft" | "published" = "published",
): Comment => ({
  id,
  change_id: 1,
  revision,
  parent_id,
  author: "reviewer",
  file: "src/main.rs",
  line: 1,
  side: "new",
  range: null,
  line_text: null,
  body: "",
  state,
  resolved: false,
  review_id: null,
  created_at: "",
  updated_at: "",
});

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

describe("threadCountByRevision", () => {
  it("counts roots per revision, ignoring replies", () => {
    const counts = threadCountByRevision([
      comment(1, 1),
      comment(2, 1),
      comment(3, 1, 1), // reply to comment 1 — rides with its thread
      comment(4, 2),
    ]);
    expect(counts.get(1)).toBe(2);
    expect(counts.get(2)).toBe(1);
    // A revision with no threads is absent (callers read with `?? 0`).
    expect(counts.get(3)).toBeUndefined();
  });

  it("counts a reviewer's drafts alongside published comments", () => {
    const counts = threadCountByRevision([
      comment(1, 2, null, "published"),
      comment(2, 2, null, "draft"),
    ]);
    expect(counts.get(2)).toBe(2);
  });

  it("is empty for no comments", () => {
    expect(threadCountByRevision([]).size).toBe(0);
  });
});

/** A comment with explicit resolution fields for the pending-state tests. */
const c = (over: Partial<Comment> & { id: number }): Comment => ({
  ...comment(over.id, over.revision ?? 1, over.parent_id ?? null, over.state),
  created_at: "2026-01-01T00:00:00Z",
  ...over,
});

describe("pendingResolved", () => {
  it("uses the published root when there are no drafts", () => {
    expect(pendingResolved(c({ id: 1, resolved: true }), [])).toBe(true);
    expect(pendingResolved(c({ id: 1, resolved: false }), [])).toBe(false);
  });

  it("lets a draft reply override the published root", () => {
    const root = c({ id: 1, resolved: false, created_at: "t0" });
    const reply = c({
      id: 2,
      parent_id: 1,
      state: "draft",
      resolved: true,
      created_at: "t1",
    });
    expect(pendingResolved(root, [reply])).toBe(true);
  });

  it("takes the newest draft when several stage decisions", () => {
    const root = c({ id: 1, resolved: false, created_at: "t0" });
    const r1 = c({
      id: 2,
      parent_id: 1,
      state: "draft",
      resolved: true,
      created_at: "t1",
    });
    const r2 = c({
      id: 3,
      parent_id: 1,
      state: "draft",
      resolved: false,
      created_at: "t2",
    });
    expect(pendingResolved(root, [r1, r2])).toBe(false);
  });

  it("reads a draft-only thread's own decision", () => {
    expect(
      pendingResolved(c({ id: 1, state: "draft", resolved: true }), []),
    ).toBe(true);
  });
});

describe("pendingUnresolvedCount", () => {
  it("counts threads open once pending drafts apply", () => {
    const comments = [
      // published resolved root + a draft reply reopening it → unresolved
      c({ id: 1, resolved: true, created_at: "t0" }),
      c({
        id: 2,
        parent_id: 1,
        state: "draft",
        resolved: false,
        created_at: "t1",
      }),
      // published unresolved root + a draft reply resolving it → resolved
      c({ id: 3, resolved: false, created_at: "t0" }),
      c({
        id: 4,
        parent_id: 3,
        state: "draft",
        resolved: true,
        created_at: "t1",
      }),
      // a plain unresolved published thread → unresolved
      c({ id: 5, resolved: false }),
    ];
    expect(pendingUnresolvedCount(comments)).toBe(2);
  });

  it("is zero with no comments", () => {
    expect(pendingUnresolvedCount([])).toBe(0);
  });
});

describe("commentCountLabel", () => {
  it("singularizes one comment", () => {
    expect(commentCountLabel(1)).toBe("1 comment");
  });
  it("pluralizes everything else", () => {
    expect(commentCountLabel(0)).toBe("0 comments");
    expect(commentCountLabel(3)).toBe("3 comments");
  });
});
