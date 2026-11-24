import { describe, expect, it } from "vitest";

import { decodeUpdateFrame } from "./documentStream";

describe("decodeUpdateFrame", () => {
  it("returns the little-endian document number and the bytes after it", () => {
    const { document, update } = decodeUpdateFrame(
      Uint8Array.of(7, 1, 0, 0, 0, 0, 0, 0, 0xaa, 0xbb),
    );
    expect(document).toBe(263);
    expect([...update]).toEqual([0xaa, 0xbb]);
  });

  it("decodes a frame that starts inside its buffer", () => {
    const bytes = Uint8Array.of(0xff, 0xff, 3, 0, 0, 0, 0, 0, 0, 0, 0xcc);
    const { document, update } = decodeUpdateFrame(bytes.subarray(2));
    expect(document).toBe(3);
    expect([...update]).toEqual([0xcc]);
  });

  it("returns an empty update for a frame of only a document number", () => {
    const { document, update } = decodeUpdateFrame(
      Uint8Array.of(1, 0, 0, 0, 0, 0, 0, 0),
    );
    expect(document).toBe(1);
    expect(update).toHaveLength(0);
  });
});
