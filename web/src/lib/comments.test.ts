import { describe, expect, it } from "vitest";
import { uiThread as ui } from "../api/fixtures/builders";
import type {
  Anchor,
  ChangeVersion,
  Draft,
  ThreadProjection,
} from "../api/types";
import type { UiThread } from "./comments";
import { revisionOf } from "./version";
import {
  anchorKind,
  assembleThreads,
  columnVersion,
  commentCountLabel,
  commentPlacement,
  pendingResolved,
  rangeSince,
  pendingUnresolvedCount,
  portThreads,
  shownAt,
  threadCountByRevision,
  threadInRange,
} from "./comments";

/** A line anchor on src/main.rs, in the version and on the line given. */
const lineAnchor = (version: ChangeVersion, line: number): Anchor => ({
  line: { version, file: "src/main.rs", at: { whole: line } },
});

/** {@link lineAnchor}, or with no line the change anchor of the revision. */
const anchor = (version: ChangeVersion, line: number | null): Anchor =>
  line === null
    ? { change: { revision: revisionOf(version) } }
    : lineAnchor(version, line);

/** A published thread anchored on src/main.rs. */
const thread = (
  over: Partial<ThreadProjection> & { id: number },
): ThreadProjection => ({
  anchor: lineAnchor("r1", 1),
  resolved: false,
  comments: [],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...over,
});

/** A reviewer draft on src/main.rs (a new thread unless `thread_id` is set). */
const draft = (over: Partial<Draft> & { id: number }): Draft => ({
  change_number: 1,
  thread_id: null,
  anchor: lineAnchor("r1", 1),
  body: "",
  resolved: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...over,
});

describe("assembleThreads", () => {
  it("merges a published thread with its reply drafts, oldest first", () => {
    const t = thread({ id: 1, created_at: "t0" });
    const d1 = draft({ id: 11, thread_id: 1, created_at: "t2", body: "later" });
    const d0 = draft({ id: 10, thread_id: 1, created_at: "t1", body: "first" });
    const [u] = assembleThreads([t], [d1, d0]);
    expect(u?.id).toBe(1);
    expect(u?.drafts.map((d) => d.id)).toEqual([10, 11]);
  });

  it("turns a new-thread draft into a draft-only UiThread", () => {
    const d = draft({ id: 20, thread_id: null, body: "new thread" });
    const [u] = assembleThreads([], [d]);
    expect(u?.id).toBeNull();
    expect(u?.comments).toEqual([]);
    expect(u?.drafts).toEqual([d]);
    // The anchor comes from the lone draft.
    expect(u?.anchor).toEqual(d.anchor);
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
      expect(commentPlacement(anchor("r2", 14), 2, "r2^")).toEqual({
        side: "new",
        line: 14,
      });
    });
    it("puts the TO old side (the parent) on the left", () => {
      expect(commentPlacement(anchor("r2^", 9), 2, "r2^")).toEqual({
        side: "old",
        line: 9,
      });
    });
    it("hides a comment from another revision", () => {
      expect(commentPlacement(anchor("r1", 5), 2, "r2^")).toBeNull();
    });
  });

  // Interdiff rM → rN: the left column is rM's own content, so a comment
  // made on rM (its new side) renders there.
  describe("rM → rN", () => {
    it("puts the TO new side on the right", () => {
      expect(commentPlacement(anchor("r3", 20), 3, "r1")).toEqual({
        side: "new",
        line: 20,
      });
    });
    it("puts the FROM revision's new side on the left", () => {
      expect(commentPlacement(anchor("r1", 7), 3, "r1")).toEqual({
        side: "old",
        line: 7,
      });
    });
    it("hides a base-side (old) comment — there is no parent column", () => {
      expect(commentPlacement(anchor("r3^", 4), 3, "r1")).toBeNull();
      expect(commentPlacement(anchor("r1^", 4), 3, "r1")).toBeNull();
    });
    it("hides a comment on a revision that is neither FROM nor TO", () => {
      expect(commentPlacement(anchor("r2", 9), 3, "r1")).toBeNull();
    });
  });

  it("never places a line-less (file-level) comment", () => {
    expect(commentPlacement(anchor("r2", null), 2, "r2^")).toBeNull();
  });
});

describe("columnVersion", () => {
  it("anchors a new-column draft to the selected revision", () => {
    expect(columnVersion("new", 3, "r3^")).toBe("r3");
    expect(columnVersion("new", 3, "r1")).toBe("r3");
  });

  it("anchors a base old-column draft to the base of the selected revision", () => {
    expect(columnVersion("old", 3, "r3^")).toBe("r3^");
  });

  it("anchors an interdiff old-column draft to the FROM revision", () => {
    expect(columnVersion("old", 3, "r1")).toBe("r1");
  });

  it("is the inverse of commentPlacement", () => {
    for (const [selected, against] of [
      [3, "r3^"],
      [3, "r1"],
    ] as const) {
      for (const column of ["old", "new"] as const) {
        const version = columnVersion(column, selected, against);
        const placed = commentPlacement(
          lineAnchor(version, 12),
          selected,
          against,
        );
        expect(placed).toEqual({ side: column, line: 12 });
      }
    }
  });
});

describe("rangeSince", () => {
  it("offers the interdiff from the thread's revision to the latest", () => {
    expect(rangeSince(anchor("r1", 5), 1, "r1^", 3)).toEqual({
      against: "r1",
      selected: 3,
    });
  });
  it("offers nothing on the latest revision", () => {
    expect(rangeSince(anchor("r3", 5), 3, "r3^", 3)).toBeNull();
  });
  it("offers nothing while that range is on screen", () => {
    expect(rangeSince(anchor("r1", 5), 3, "r1", 3)).toBeNull();
  });
  it("offers a change-level thread the range too", () => {
    expect(rangeSince(anchor("r1", null), 1, "r1^", 3)).toEqual({
      against: "r1",
      selected: 3,
    });
  });
  it("offers nothing to an old-side thread, which the range would hide", () => {
    expect(rangeSince(anchor("r1^", 5), 1, "r1^", 3)).toBeNull();
  });
});

describe("threadCountByRevision", () => {
  it("counts UiThreads per revision", () => {
    const counts = threadCountByRevision([
      ui({ id: 1, anchor: lineAnchor("r1", 1) }),
      ui({ id: 2, anchor: lineAnchor("r1^", 1) }),
      ui({ id: 3, anchor: lineAnchor("r2", 1) }),
    ]);
    expect(counts.get(1)).toBe(2);
    expect(counts.get(2)).toBe(1);
    // A revision with no threads is absent (callers read with `?? 0`).
    expect(counts.get(3)).toBeUndefined();
  });

  it("counts a reviewer's draft-only threads alongside published ones", () => {
    const counts = threadCountByRevision([
      ui({ id: 1, anchor: lineAnchor("r2", 1) }),
      ui({ id: null, anchor: lineAnchor("r2", 1) }),
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

  it("takes the newest draft when several draft decisions", () => {
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

  it("reads a draft-only thread's own draft decision", () => {
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

// The three anchor kinds answer the range question differently. Only a
// line anchor can lose its column.
describe("anchorKind", () => {
  const fileAnchor: Anchor = { file: { revision: 0, file: "src/main.rs" } };

  it("returns the arm the anchor is on", () => {
    expect(anchorKind({ change: { revision: 0 } })).toBe("change");
    expect(anchorKind(fileAnchor)).toBe("file");
    expect(anchorKind(lineAnchor("r0", 3))).toBe("line");
  });

  it("shows a file thread in every range, its own or not", () => {
    expect(threadInRange(fileAnchor, 0, "r0^")).toBe(true);
    expect(threadInRange(fileAnchor, 3, "r2")).toBe(true);
    // A line thread on the same revision has no column in that range.
    expect(threadInRange(anchor("r0", 3), 3, "r2")).toBe(false);
  });
});

describe("portThreads", () => {
  // The server lists TO's places before FROM's.
  const ported = [
    { thread_id: 1, anchor: lineAnchor("r2", 8) },
    { thread_id: 1, anchor: lineAnchor("r1", 6) },
  ];
  const only = (threads: UiThread[]): UiThread => {
    const t = threads[0];
    if (t === undefined || threads.length !== 1) throw new Error("one thread");
    return t;
  };

  it("shows a hidden thread at its ported place", () => {
    const t = ui({ id: 1, anchor: lineAnchor("r0", 5) });
    const shown = only(portThreads([t], ported, 2, "r2^"));
    expect(shown.ported).toEqual(lineAnchor("r2", 8));
    expect(shownAt(shown)).toEqual(shown.ported);
    expect(shown.anchor).toEqual(lineAnchor("r0", 5));
  });

  it("keeps a thread the range already shows", () => {
    const t = ui({ id: 1, anchor: lineAnchor("r2", 5) });
    const shown = only(portThreads([t], ported, 2, "r2^"));
    expect(shown.ported).toBeUndefined();
    expect(shownAt(shown)).toBe(shown.anchor);
  });

  it("takes the first ported place the range shows", () => {
    const t = ui({ id: 1, anchor: lineAnchor("r0", 5) });
    expect(only(portThreads([t], ported, 2, "r1")).ported).toEqual(
      lineAnchor("r2", 8),
    );
    // Vs base, FROM's tree is not shown, so only TO's place counts.
    expect(only(portThreads([t], ported.slice(1), 2, "r2^")).ported).toBe(
      undefined,
    );
    expect(only(portThreads([t], ported.slice(1), 2, "r1")).ported).toEqual(
      lineAnchor("r1", 6),
    );
  });

  it("leaves a thread the server did not port, and every draft-only thread", () => {
    const missing = ui({ id: 9, anchor: lineAnchor("r0", 5) });
    const draftOnly = ui({ id: null, anchor: lineAnchor("r0", 1) });
    for (const t of portThreads([missing, draftOnly], ported, 2, "r2^")) {
      expect(t.ported).toBeUndefined();
    }
  });
});
