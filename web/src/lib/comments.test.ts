import { describe, expect, it } from "vitest";
import type { Draft, Thread } from "../api/types";
import type { CommentAnchor, UiThread } from "./comments";
import {
  assembleThreads,
  commentCountLabel,
  commentPlacement,
  draftAnchor,
  pendingResolved,
  pendingUnresolvedCount,
  revisionActivity,
  threadCountByRevision,
} from "./comments";

const anchor = (revision: number, side: "old" | "new", line: number | null) =>
  ({ revision, side, line }) satisfies CommentAnchor;

/** A published thread anchored on src/main.rs; only the fields the test
 * exercises are spelled out, the rest take sensible defaults. */
const thread = (over: Partial<Thread> & { id: number }): Thread => ({
  change_id: 1,
  revision: 1,
  file: "src/main.rs",
  line: 1,
  side: "new",
  range: null,
  line_text: null,
  resolved: false,
  comments: [],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...over,
});

/** A reviewer draft on src/main.rs (a new thread unless `thread_id` is set). */
const draft = (over: Partial<Draft> & { id: number }): Draft => ({
  change_id: 1,
  thread_id: null,
  revision: 1,
  file: "src/main.rs",
  line: 1,
  side: "new",
  range: null,
  line_text: null,
  body: "",
  resolved: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...over,
});

/** A UiThread built inline for the pending-state/count tests. */
const ui = (over: Partial<UiThread> & { id: number | null }): UiThread => ({
  revision: 1,
  file: "src/main.rs",
  line: 1,
  side: "new",
  range: null,
  line_text: null,
  resolved: false,
  comments: [],
  drafts: [],
  created_at: "2026-01-01T00:00:00Z",
  ...over,
});

describe("assembleThreads", () => {
  it("merges a published thread with its reply drafts, oldest first", () => {
    const t = thread({ id: 1, created_at: "t0" });
    const d1 = draft({ id: 11, thread_id: 1, created_at: "t2", body: "later" });
    const d0 = draft({ id: 10, thread_id: 1, created_at: "t1", body: "first" });
    const [u] = assembleThreads([t], [d1, d0]);
    expect(u?.id).toBe(1);
    // Reply drafts collected onto the thread, sorted oldest-first.
    expect(u?.drafts.map((d) => d.id)).toEqual([10, 11]);
  });

  it("turns a new-thread draft into a draft-only UiThread", () => {
    const d = draft({ id: 20, thread_id: null, body: "new thread" });
    const [u] = assembleThreads([], [d]);
    expect(u?.id).toBeNull();
    expect(u?.comments).toEqual([]);
    expect(u?.drafts).toEqual([d]);
    // The anchor comes from the lone draft.
    expect(u?.line).toBe(d.line);
  });

  it("sorts published and draft-only threads together by creation time", () => {
    const tA = thread({ id: 1, created_at: "t1" });
    const tB = thread({ id: 2, created_at: "t3" });
    const dOnly = draft({ id: 30, thread_id: null, created_at: "t2" });
    const assembled = assembleThreads([tB, tA], [dOnly]);
    expect(assembled.map((u) => u.id)).toEqual([1, null, 2]);
  });
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
  it("counts UiThreads per revision", () => {
    const counts = threadCountByRevision([
      ui({ id: 1, revision: 1 }),
      ui({ id: 2, revision: 1 }),
      ui({ id: 3, revision: 2 }),
    ]);
    expect(counts.get(1)).toBe(2);
    expect(counts.get(2)).toBe(1);
    // A revision with no threads is absent (callers read with `?? 0`).
    expect(counts.get(3)).toBeUndefined();
  });

  it("counts a reviewer's draft-only threads alongside published ones", () => {
    const counts = threadCountByRevision([
      ui({ id: 1, revision: 2 }),
      ui({ id: null, revision: 2 }),
    ]);
    expect(counts.get(2)).toBe(2);
  });

  it("is empty for no threads", () => {
    expect(threadCountByRevision([]).size).toBe(0);
  });
});

describe("pendingResolved", () => {
  it("uses the published resolution when there are no drafts", () => {
    expect(pendingResolved(ui({ id: 1, resolved: true }))).toBe(true);
    expect(pendingResolved(ui({ id: 1, resolved: false }))).toBe(false);
  });

  it("lets a draft reply override the published resolution", () => {
    const t = ui({
      id: 1,
      resolved: false,
      drafts: [draft({ id: 2, thread_id: 1, resolved: true })],
    });
    expect(pendingResolved(t)).toBe(true);
  });

  it("takes the newest draft when several stage decisions", () => {
    // assembleThreads keeps drafts oldest-first, so the last one wins.
    const t = ui({
      id: 1,
      resolved: false,
      drafts: [
        draft({ id: 2, thread_id: 1, resolved: true, created_at: "t1" }),
        draft({ id: 3, thread_id: 1, resolved: false, created_at: "t2" }),
      ],
    });
    expect(pendingResolved(t)).toBe(false);
  });

  it("reads a draft-only thread's own staged decision", () => {
    const t = ui({
      id: null,
      drafts: [draft({ id: 2, thread_id: null, resolved: true })],
    });
    expect(pendingResolved(t)).toBe(true);
  });
});

describe("pendingUnresolvedCount", () => {
  it("counts threads open once pending drafts apply", () => {
    const threads = [
      // published resolved + a draft reply reopening it → unresolved
      ui({
        id: 1,
        resolved: true,
        drafts: [draft({ id: 11, thread_id: 1, resolved: false })],
      }),
      // published unresolved + a draft reply resolving it → resolved
      ui({
        id: 2,
        resolved: false,
        drafts: [draft({ id: 12, thread_id: 2, resolved: true })],
      }),
      // a plain unresolved published thread → unresolved
      ui({ id: 3, resolved: false }),
    ];
    expect(pendingUnresolvedCount(threads)).toBe(2);
  });

  it("is zero with no threads", () => {
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

describe("revisionActivity", () => {
  it("counts threads, drafts and unresolved at the pinned revision only", () => {
    const a = revisionActivity(
      [
        thread({ id: 1, revision: 2, resolved: false }),
        thread({ id: 2, revision: 2, resolved: true }),
        thread({ id: 3, revision: 1, resolved: false }),
      ],
      [draft({ id: 11, revision: 2 }), draft({ id: 12, revision: 1 })],
      2,
    );
    // unresolved excludes the resolved thread; the rev-1 thread/draft are out.
    expect(a).toEqual({ threads: 2, drafts: 1, unresolved: 1 });
  });

  it("is all-zero for a revision with no activity", () => {
    expect(revisionActivity([], [], 0)).toEqual({
      threads: 0,
      drafts: 0,
      unresolved: 0,
    });
  });
});
