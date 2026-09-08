import { describe, expect, it } from "vitest";

import type { StreamMessage } from "../types";
import { changeId } from "./builders";
import { mockAppend, mockOpenStream } from "./stream";

describe("mock stream", () => {
  it("projects a change on subscribe, then delivers live appends", () => {
    const got: StreamMessage[] = [];
    const handle = mockOpenStream((m) => got.push(m));

    // Change 11 alone: revisions + a review + threads.
    handle.subscribe({
      query: { repo: 1, change_id: changeId("I3f2d8a91c0b7e514") },
    });
    expect(got).toHaveLength(1);
    const snap = got[0];
    expect(snap && "projection" in snap && snap.projection.id).toBe(11);
    expect(
      snap && "projection" in snap && snap.projection.revisions.length,
    ).toBeGreaterThan(0);

    mockAppend(11, "t-live", {
      kind: "lifecycle",
      payload: { action: "abandoned", message: null },
    });
    expect(got).toHaveLength(2);
    const live = got[1];
    expect(live && "entry" in live && live.entry.kind).toBe("lifecycle");

    handle.close();
    mockAppend(11, "t-after", {
      kind: "lifecycle",
      payload: { action: "reopened", message: null },
    });
    // No delivery after close.
    expect(got).toHaveLength(2);
  });

  it("projects every change the query picks, and no other", () => {
    const got: StreamMessage[] = [];
    const handle = mockOpenStream((m) => got.push(m));
    handle.subscribe({ query: { repo: 1, tag: ["session-id=auth-rotation"] } });
    expect(got.map((m) => ("projection" in m ? m.projection.id : -1))).toEqual([
      10, 11, 12,
    ]);
    handle.close();
  });
});
