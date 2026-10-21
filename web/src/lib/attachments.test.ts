import { describe, expect, it } from "vitest";
import { attachmentFile, renderedKind, type ReviewFile } from "./attachments";

const attached = (name: string, binary: boolean): ReviewFile => ({
  path: `/attachments/${name}`,
  status: "added",
  attachment: { name, hash: "a".repeat(64), revision: 0 },
  binary,
  additions: 0,
  deletions: 0,
  old_total: 0,
  new_total: 0,
  hunks: [],
});

describe("renderedKind", () => {
  it("reads the kind from the extension, in any case", () => {
    expect(renderedKind(attached("plan/a.MD", false))).toBe("markdown");
    expect(renderedKind(attached("api/index.html", false))).toBe("html");
    expect(renderedKind(attached("shot.png", true))).toBe("image");
  });

  it("shows another extension as a file does", () => {
    expect(renderedKind(attached("out.log", false))).toBeNull();
  });

  it("shows Markdown sent without lines as a file does", () => {
    expect(renderedKind(attached("huge.md", true))).toBeNull();
  });

  it("takes no extension from a directory name", () => {
    expect(renderedKind(attached("v1.md/notes", false))).toBeNull();
  });
});

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
