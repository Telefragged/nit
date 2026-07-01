import { describe, expect, it, vi } from "vitest";

import { ASYNC_TIMEOUT_MS } from "../test-setup";
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

    // vi.waitFor keeps its own 1000ms default — testing-library's config
    // doesn't reach it, so size it for load the same way (src/test-setup).
    await vi.waitFor(
      () => {
        expect(got.some((m) => "snapshot" in m && m.snapshot.id === 30)).toBe(
          true,
        );
      },
      { timeout: ASYNC_TIMEOUT_MS },
    );

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
