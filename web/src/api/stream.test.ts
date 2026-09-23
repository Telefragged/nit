import { describe, expect, it } from "vitest";

import { mockAppend } from "./fixtures/stream";
import { openStream } from "./stream";
import type { StreamMessage } from "./types";

describe("openStream (mock mode)", () => {
  it("projects on subscribe, then goes live", () => {
    const got: StreamMessage[] = [];
    const handle = openStream((m) => {
      got.push(m);
    });
    // Change 30 is the only change under its session.
    handle.subscribe({ query: { repo: 2, tag: ["session-id=ci-cache"] } });
    expect(got.map((m) => "projection" in m && m.projection.id)).toEqual([30]);

    mockAppend(30, "t-live", {
      kind: "lifecycle",
      payload: { action: "abandoned", message: null },
    });
    expect(got).toHaveLength(2);
    const last = got.at(-1);
    expect(last && "entry" in last && last.entry.change_number).toBe(30);

    handle.close();
  });
});
