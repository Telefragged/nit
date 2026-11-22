import { describe, expect, it } from "vitest";
import { elementAt, elementPath } from "./framePicker";

const page = () =>
  new DOMParser().parseFromString(
    `<!doctype html><body>
      <div><p>one</p></div>
      <div><p>two</p><p>three</p><my-card>card</my-card></div>
    </body>`,
    "text/html",
  );

describe("elementPath", () => {
  it("counts an element among the siblings of its name, from 1", () => {
    const doc = page();
    const at = (selector: string) => {
      const el = doc.querySelector(selector);
      if (!el) throw new Error(selector);
      return elementPath(el);
    };
    expect(at("body")).toBe("/html/body");
    expect(at("p")).toBe("/html/body/div/p");
    expect(at("div + div p + p")).toBe("/html/body/div[2]/p[2]");
    expect(at("my-card")).toBe("/html/body/div[2]/my-card");
  });

  it("stops above an element whose tag name is not a plain name", () => {
    const doc = page();
    const odd = doc.createElementNS("http://www.w3.org/1999/xhtml", "a.b");
    const inner = doc.createElement("span");
    odd.append(inner);
    doc.body.append(odd);
    expect(elementPath(inner)).toBe("/html/body");
  });
});

describe("elementAt", () => {
  it("is the inverse of elementPath", () => {
    const doc = page();
    for (const el of doc.querySelectorAll("*"))
      expect(elementAt(doc, elementPath(el))).toBe(el);
  });

  it("finds nothing at a path that the page does not have", () => {
    const doc = page();
    expect(elementAt(doc, "/html/body/div[3]")).toBeNull();
    expect(elementAt(doc, "/html/body/p")).toBeNull();
    expect(elementAt(doc, "/body")).toBeNull();
  });
});
