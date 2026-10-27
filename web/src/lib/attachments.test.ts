import { describe, expect, it } from "vitest";
import { attachmentFile } from "./attachments";

describe("attachmentFile", () => {
  const attachment = { name: "out.log", hash: "a".repeat(64), revision: 0 };

  it("shows text as one all-added hunk, without a line for the last newline", () => {
    const file = attachmentFile(attachment, "one\ntwo\n");
    expect(file.binary).toBe(false);
    expect(file.hunks[0]?.lines.map((l) => [l.new, l.text])).toEqual([
      [1, "one"],
      [2, "two"],
    ]);
  });

  it("shows no text as a binary file", () => {
    const file = attachmentFile(attachment, null);
    expect(file.binary).toBe(true);
    expect(file.hunks).toEqual([]);
  });
});
