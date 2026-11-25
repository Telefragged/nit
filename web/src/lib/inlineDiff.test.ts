import type { ElementContent } from "hast";
import { describe, expect, it } from "vitest";
import { parseBlocks } from "./document";
import { inlineRow } from "./inlineDiff";

function marked(node: ElementContent): string {
  if (node.type === "text") return node.value;
  if (node.type !== "element") return "";
  const classes = [node.properties.className ?? []].flat();
  if (classes.includes("doc-deletion"))
    return `[-${String(node.properties.dataRemoved)}-]`;
  const inner = node.children.map(marked).join("");
  return classes.includes("doc-inserted") ? `{+${inner}+}` : inner;
}

function row(old: string, now: string) {
  const blocks = inlineRow(
    parseBlocks(old),
    parseBlocks(now),
    old.split("\n"),
    now.split("\n"),
  );
  return blocks.map(
    ({ block, side, change }) =>
      (change === "" ? "" : side === "old" ? "- " : "+ ") +
      marked(block.node).trim(),
  );
}

describe("inlineRow", () => {
  it("marks an insertion inside the text", () => {
    expect(row("One three.", "One two three.")).toEqual(["One {+two +}three."]);
  });

  it("puts a marker in place of a deletion", () => {
    expect(row("One two three.", "One three.")).toEqual(["One [-two-]three."]);
  });

  it("puts the marker of a replaced text before the new text", () => {
    expect(row("The old token is here.", "The new token is here.")).toEqual([
      "The [-old-]{+new+} token is here.",
    ]);
  });

  it("marks the text that Markdown renders from an insertion", () => {
    expect(row("A plain word here.", "A **bold** plain word here.")).toEqual([
      "A {+bold+}{+ +}plain word here.",
    ]);
  });

  it("shows a block that the diff adds whole, next to a changed block", () => {
    expect(
      row(
        "- The lookup trims the token.",
        "- The lookup trims the presented token.\n- A rebase renamed it.",
      ),
    ).toEqual([
      "The lookup trims the {+presented +}token.",
      "+ A rebase renamed it.",
    ]);
  });

  it("shows a block that the diff removes whole, above the next block", () => {
    expect(
      row(
        "First paragraph, gone.\n\nSecond paragraph stays here.",
        "Second paragraph stays right here.",
      ),
    ).toEqual([
      "- First paragraph, gone.",
      "Second paragraph stays {+right +}here.",
    ]);
  });

  it("puts a deletion at the end of a block after its last text", () => {
    expect(row("Keep this, drop that.", "Keep this")).toEqual([
      "Keep this[-, drop that.-]",
    ]);
  });

  it("shows the blocks whole when the diff replaces most of the text", () => {
    expect(row("Alpha beta gamma.", "Nothing in common, 1234.")).toEqual([
      "- Alpha beta gamma.",
      "+ Nothing in common, 1234.",
    ]);
  });

  it("shows the blocks whole when one side has no block", () => {
    expect(row("", "Added.")).toEqual(["+ Added."]);
    expect(row("Gone.", "")).toEqual(["- Gone."]);
  });

  it("shows a code block whole", () => {
    const fence = (code: string) => "```\n" + code + "\n```";
    expect(row(fence("let one = 1;"), fence("let one = 2;"))).toEqual([
      "- let one = 1;",
      "+ let one = 2;",
    ]);
  });
});
