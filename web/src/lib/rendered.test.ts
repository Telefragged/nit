import { describe, expect, it } from "vitest";
import { add, ctx, del } from "../api/fixtures/builders";
import { attachmentFile, type ReviewFile } from "./attachments";
import { renderedKind, sideText } from "./rendered";

const file = (path: string, binary: boolean): ReviewFile => ({
  path,
  status: "added",
  binary,
  additions: 0,
  deletions: 0,
  old_total: 0,
  new_total: 0,
  hunks: [],
});

const attached = (name: string, binary: boolean) =>
  attachmentFile(
    { name, hash: "a".repeat(64), revision: 0 },
    binary ? null : "",
  );

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

  it("renders an attachment's HTML page from its URL, without lines", () => {
    expect(renderedKind(attached("huge.html", true))).toBe("html");
  });

  it("takes no extension from a directory name", () => {
    expect(renderedKind(attached("v1.md/notes", false))).toBeNull();
  });

  it("renders the commit's Markdown and HTML from their lines", () => {
    expect(renderedKind(file("docs/a.md", false))).toBe("markdown");
    expect(renderedKind(file("site/index.html", false))).toBe("html");
    expect(renderedKind(file("site/index.html", true))).toBeNull();
  });

  it("shows the commit's images as files do", () => {
    expect(renderedKind(file("docs/flow.png", true))).toBeNull();
  });
});

describe("sideText", () => {
  const lines = [ctx(1, 1, "# Title"), del(2, "old"), add(2, "new")];

  it("joins the lines of one side", () => {
    expect(sideText(lines, "new")).toBe("# Title\nnew");
    expect(sideText(lines, "old")).toBe("# Title\nold");
  });
});
