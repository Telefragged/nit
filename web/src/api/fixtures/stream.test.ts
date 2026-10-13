import { describe, expect, it } from "vitest";

import type { StreamMessage } from "../types";
import { changeId } from "./builders";
import { logFor, mockAppend, mockOpenStream } from "./stream";

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

  it("sends the entries past the held positions", () => {
    const got: StreamMessage[] = [];
    const handle = mockOpenStream((m) => got.push(m));
    const last = logFor(11).length - 1;
    handle.subscribe({ query: { change: 11 }, positions: { 11: last } });
    expect(got.map((m) => "entry" in m && m.entry.position)).toEqual([last]);
    handle.close();
  });

  it("sends left when a change stops matching", () => {
    const got: StreamMessage[] = [];
    const handle = mockOpenStream((m) => got.push(m));
    handle.subscribe({
      query: { change: 12, status: ["pending", "commented", "approved"] },
    });
    expect(got.map((m) => "projection" in m && m.projection.id)).toEqual([12]);
    mockAppend(12, "t-live", {
      kind: "lifecycle",
      payload: { action: "abandoned", message: null },
    });
    expect(got.at(-1)).toEqual({ left: 12 });
    handle.close();
  });
});
