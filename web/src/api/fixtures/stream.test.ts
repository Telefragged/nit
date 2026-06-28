import { describe, expect, it } from "vitest";

import type { StreamMsg } from "../types";
import { mockAppend, mockOpenStream } from "./stream";

describe("mock stream", () => {
  it("snapshots a change on subscribe, then delivers live appends", () => {
    const got: StreamMsg[] = [];
    const handle = mockOpenStream((m) => got.push(m));

    handle.add([11]); // change 11: revisions + a review + threads
    expect(got).toHaveLength(1);
    const snap = got[0];
    expect(snap && "snapshot" in snap && snap.snapshot.id).toBe(11);
    expect(
      snap && "snapshot" in snap && snap.snapshot.revisions.length,
    ).toBeGreaterThan(0);

    mockAppend(11, "t-live", {
      kind: "lifecycle",
      payload: { action: "abandoned", revision: null, message: null },
    });
    expect(got).toHaveLength(2);
    const live = got[1];
    expect(live && "entry" in live && live.entry.kind).toBe("lifecycle");

    handle.close();
    mockAppend(11, "t-after", {
      kind: "lifecycle",
      payload: { action: "reopened", revision: null, message: null },
    });
    // No delivery after close.
    expect(got).toHaveLength(2);
  });

  it("only snapshots subscribed changes", () => {
    const got: StreamMsg[] = [];
    const handle = mockOpenStream((m) => got.push(m));
    handle.add([20]);
    expect(got).toHaveLength(1);
    expect(got[0] && "snapshot" in got[0] && got[0].snapshot.id).toBe(20);
    handle.close();
  });
});
