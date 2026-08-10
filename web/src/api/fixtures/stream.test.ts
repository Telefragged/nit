import { describe, expect, it } from "vitest";

import type { StreamMessage } from "../types";
import { mockAppend, mockOpenStream } from "./stream";

describe("mock stream", () => {
  it("projects a change on subscribe, then delivers live appends", () => {
    const got: StreamMessage[] = [];
    const handle = mockOpenStream((m) => got.push(m));

    handle.add([11]); // change 11: revisions + a review + threads
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

  it("only projects subscribed changes", () => {
    const got: StreamMessage[] = [];
    const handle = mockOpenStream((m) => got.push(m));
    handle.add([20]);
    expect(got).toHaveLength(1);
    expect(got[0] && "projection" in got[0] && got[0].projection.id).toBe(20);
    handle.close();
  });
});
