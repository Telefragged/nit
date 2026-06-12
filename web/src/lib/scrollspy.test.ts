import { describe, expect, it } from "vitest";
import { activeIndexAt } from "./scrollspy";

describe("activeIndexAt", () => {
  it("returns null for an empty section list", () => {
    expect(activeIndexAt([], 116)).toBe(null);
  });

  it("returns null while the page is above the first section", () => {
    // Every top below the threshold line — nothing has reached it yet,
    // matching the no-highlight state on load.
    expect(activeIndexAt([200, 900, 1600], 116)).toBe(null);
  });

  it("treats a section exactly at the threshold as active", () => {
    expect(activeIndexAt([116, 900], 116)).toBe(0);
  });

  it("picks the last section at or above the threshold", () => {
    // Sections 0–2 have scrolled past the line; like a sticky header,
    // the latest one to cross it is current.
    expect(activeIndexAt([-800, -300, 50, 700], 116)).toBe(2);
  });

  it("leaves a section just below the threshold inactive (+1 fudge)", () => {
    // The component derives threshold as scrollMarginTop + 1 so a
    // fractional scroll landing (e.g. top 116.4 with a 116px margin)
    // still counts as arrived…
    expect(activeIndexAt([-300, 116.4], 116 + 1)).toBe(1);
    // …while a section meaningfully short of the line does not.
    expect(activeIndexAt([-300, 116 + 1 + 0.5], 116 + 1)).toBe(0);
  });
});
