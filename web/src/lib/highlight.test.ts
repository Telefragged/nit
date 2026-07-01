// markTextRange walks rendered DOM text nodes, so raw-text offsets must
// survive entity escaping, hljs token spans, and stacking with earlier
// marks — the offsets the comment-range contract anchors to.

import { describe, expect, it } from "vitest";
import { highlightLine, markIntraline, markTextRange } from "./highlight";

function parse(html: string): DocumentFragment {
  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  return tpl.content;
}

const markedText = (root: DocumentFragment, selector: string): string =>
  [...root.querySelectorAll(selector)].map((el) => el.textContent).join("");

describe("markTextRange", () => {
  it("wraps the raw-text slice across entities and token spans", () => {
    const raw = 'if a < b && c > "x" {';
    const html = highlightLine(raw, "rust");
    const root = parse(markTextRange(html, 3, 16, "comment-range"));
    expect(markedText(root, ".comment-range")).toBe(raw.slice(3, 16));
    expect(root.textContent).toBe(raw);
  });

  it("stacks with an intraline mark, each wrapping its own chars", () => {
    const raw = "let value = compute(input);";
    let html = highlightLine(raw, "rust");
    html = markIntraline(html, 4, 9);
    html = markTextRange(html, 6, 19, "comment-range");
    const root = parse(html);
    expect(markedText(root, ".intraline")).toBe(raw.slice(4, 9));
    expect(markedText(root, ".comment-range")).toBe(raw.slice(6, 19));
    expect(root.textContent).toBe(raw);
  });

  it("clamps to the text and ignores empty windows", () => {
    const raw = "short";
    const html = highlightLine(raw, null);
    expect(parse(markTextRange(html, 2, 99, "m")).textContent).toBe(raw);
    expect(markTextRange(html, 3, 3, "m")).toBe(html);
  });
});
