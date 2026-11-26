import type { ElementContent } from "hast";
import { describe, expect, it } from "vitest";
import { parseBlocks } from "./document";

function shape(node: ElementContent): string {
  if (node.type === "text") return node.value.trim();
  if (node.type !== "element") return "";
  const inner = node.children.map(shape).filter(Boolean).join(" ");
  if (node.tagName === "span") return inner;
  const open = node.properties.open ? " open" : "";
  return `<${node.tagName}${open}>${inner}</${node.tagName}>`;
}

const shapes = (text: string) =>
  parseBlocks(text).map((block) => shape(block.node));

describe("remarkDetails", () => {
  it("renders a details block as one collapsible group", () => {
    const text = [
      "Before.",
      "",
      "<details>",
      "<summary>The title</summary>",
      "",
      "One **bold** line.",
      "",
      "- an item",
      "",
      "</details>",
      "",
      "After.",
    ].join("\n");
    expect(shapes(text)).toEqual([
      "<p>Before.</p>",
      "<details><summary>The title</summary> <p>One <strong>bold</strong> line.</p> <ul><li>an item</li></ul></details>",
      "<p>After.</p>",
    ]);
    expect(parseBlocks(text)[1]?.lines).toEqual([3, 10]);
  });

  it("keeps the open attribute, and gives a group without a summary a title", () => {
    expect(shapes("<details open>\n\nText.\n\n</details>")).toEqual([
      "<details open><summary>Details</summary> <p>Text.</p></details>",
    ]);
  });

  it("puts a group inside a group", () => {
    const text =
      "<details>\n<summary>Outer</summary>\n\n<details>\n<summary>Inner</summary>\n\nText.\n\n</details>\n\n</details>";
    expect(shapes(text)).toEqual([
      "<details><summary>Outer</summary> <details><summary>Inner</summary> <p>Text.</p></details></details>",
    ]);
  });

  it("renders no other HTML, also inside a summary", () => {
    expect(
      shapes(
        '<details onclick="x()">\n\nText.\n\n</details>\n\n<script>x()</script>\n\n<div>raw</div>',
      ),
    ).toEqual(["<p>Text.</p>"]);
    expect(
      shapes(
        "<details>\n<summary><img src=x onerror=y>T</summary>\n\n</details>",
      ),
    ).toEqual(["<details><summary><img src=x onerror=y>T</summary></details>"]);
  });
});
