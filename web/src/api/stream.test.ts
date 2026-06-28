import { describe, expect, it, vi } from "vitest";

import { mockAppend } from "./fixtures/stream";
import { openStream } from "./stream";
import type { StreamMsg } from "./types";

describe("openStream (mock mode)", () => {
  it("queues subscriptions until the mock loads, then snapshots and goes live", async () => {
    const got: StreamMsg[] = [];
    const handle = openStream((m) => {
      got.push(m);
    });
    // add() is called before the lazy mock import resolves — it must queue.
    handle.add([30]);

    await vi.waitFor(() => {
      expect(got.some((m) => "snapshot" in m && m.snapshot.id === 30)).toBe(
        true,
      );
    });

    const before = got.length;
    mockAppend(30, "t-live", {
      kind: "lifecycle",
      payload: { action: "abandoned", revision: null, message: null },
    });
    expect(got).toHaveLength(before + 1);
    const last = got.at(-1);
    expect(last && "entry" in last && last.entry.change_id).toBe(30);

    handle.close();
  });
});
