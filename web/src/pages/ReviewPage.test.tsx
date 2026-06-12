// Review page collapse behavior, rendered against the mock fixtures
// (VITE_MOCK is set by the vitest config). Change 11 at ?against=base is
// the full r2 diff: /COMMIT_MSG, src/auth/rotate.rs, src/auth/store.rs,
// tests/rotation.rs — i.e. file-0 .. file-3.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ReviewPage from "./ReviewPage";

// No vitest globals → testing-library cannot auto-cleanup; without this,
// earlier renders stay mounted and their window keydown listeners (and
// duplicate file-N ids) bleed into later tests.
afterEach(cleanup);

/** Every scrollIntoView call: which element, and whether that element's
 * diff body was already in the DOM when the call happened. The latter is
 * the regression guard for the collapse pitfall — a scroll issued before
 * the expansion commit would see (and target) the pre-reflow layout. */
let scrollCalls: Array<{ id: string; expandedAtCall: boolean }>;

beforeEach(() => {
  scrollCalls = [];
  Element.prototype.scrollIntoView = function (this: Element) {
    scrollCalls.push({
      id: this.id,
      expandedAtCall: this.querySelector(".diff-table") !== null,
    });
  };
});

function renderReview(url = "/changes/11?against=base") {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/changes/:id" element={<ReviewPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const section = (i: number): HTMLElement => {
  const el = document.getElementById(`file-${i}`);
  expect(el).not.toBeNull();
  return el!;
};
const isExpanded = (el: HTMLElement): boolean =>
  el.querySelector(".file-header")?.getAttribute("aria-expanded") === "true";

/** Wait for the diff to load: the rail item for store.rs is rendered. */
const railItem = (path: string) => screen.findByTitle(path);

describe("collapsed-by-default file sections", () => {
  it("starts with every file collapsed except the commit message", async () => {
    renderReview();
    await railItem("src/auth/store.rs");

    expect(isExpanded(section(0))).toBe(true); // /COMMIT_MSG
    expect(section(0).querySelector(".diff-table")).not.toBeNull();
    for (const i of [1, 2, 3]) {
      expect(isExpanded(section(i))).toBe(false);
      // aria matches reality: no diff body rendered while collapsed.
      expect(section(i).querySelector(".diff-table")).toBeNull();
    }
  });

  it("toggles a section from its header without scrolling", async () => {
    renderReview();
    await railItem("src/auth/store.rs");

    const header = section(1).querySelector(".file-header")!;
    fireEvent.click(header);
    expect(isExpanded(section(1))).toBe(true);
    fireEvent.click(header);
    expect(isExpanded(section(1))).toBe(false);
    expect(scrollCalls).toEqual([]);
  });

  it("rail click expands the target and scrolls only after the expansion is committed", async () => {
    renderReview();
    await railItem("src/auth/store.rs");

    // Mixed state: commit message (above the target) expanded, the two
    // files in between and the target collapsed — the layout-shift case.
    expect(isExpanded(section(0))).toBe(true);
    expect(isExpanded(section(1))).toBe(false);
    expect(isExpanded(section(2))).toBe(false);

    fireEvent.click(screen.getByTitle("src/auth/store.rs"));

    // Exactly one scroll, on the clicked file's section, and the section
    // already carried its expanded body when the call was issued.
    expect(scrollCalls).toEqual([{ id: "file-2", expandedAtCall: true }]);
    expect(isExpanded(section(2))).toBe(true);
    // Only the target expanded; its collapsed neighbor stayed collapsed.
    expect(isExpanded(section(1))).toBe(false);
  });

  it("the ] key reveals the next file like a rail click", async () => {
    renderReview();
    await railItem("src/auth/store.rs");

    fireEvent.keyDown(window, { key: "]" }); // → file-0 (already expanded)
    fireEvent.keyDown(window, { key: "]" }); // → file-1 (was collapsed)

    expect(scrollCalls).toEqual([
      { id: "file-0", expandedAtCall: true },
      { id: "file-1", expandedAtCall: true },
    ]);
    expect(isExpanded(section(1))).toBe(true);
  });

  it("expand all / collapse all flips every section", async () => {
    renderReview();
    await railItem("src/auth/store.rs");

    fireEvent.click(screen.getByRole("button", { name: "expand all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "collapse all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(false);
    // Bulk toggling never scrolls.
    expect(scrollCalls).toEqual([]);
  });
});

// Collapsing the section that hosts the open inline editor unmounts it,
// which is a discard path: it must route through confirmDiscard (i.e.
// window.confirm while dirty) like every other editor teardown.
describe("collapse with an open dirty comment editor", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Expand rotate.rs (file-1), open the inline draft editor on its first
   * commentable line and type into it, leaving the draft dirty. */
  async function openDirtyEditor() {
    renderReview();
    await railItem("src/auth/store.rs");
    fireEvent.click(section(1).querySelector(".file-header")!);
    fireEvent.click(section(1).querySelector(".line-row.clickable")!);
    fireEvent.change(section(1).querySelector("textarea")!, {
      target: { value: "half-typed nit" },
    });
  }

  it("declined header collapse keeps the file expanded and the editor mounted", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    await openDirtyEditor();

    fireEvent.click(section(1).querySelector(".file-header")!);

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(isExpanded(section(1))).toBe(true);
    expect(section(1).querySelector("textarea")).not.toBeNull();
  });

  it("accepted header collapse discards the draft and collapses the section", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    await openDirtyEditor();

    fireEvent.click(section(1).querySelector(".file-header")!);

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(isExpanded(section(1))).toBe(false);
    // Re-expanding must not resurrect an empty editor at the stale anchor.
    fireEvent.click(section(1).querySelector(".file-header")!);
    expect(isExpanded(section(1))).toBe(true);
    expect(section(1).querySelector("textarea")).toBeNull();
  });

  it("collapse all routes through the same guard", async () => {
    const confirm = vi
      .spyOn(window, "confirm")
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true);
    await openDirtyEditor();
    // Expanding is never a discard: no prompt for expand all.
    fireEvent.click(screen.getByRole("button", { name: "expand all" }));
    expect(confirm).not.toHaveBeenCalled();

    // Declined: nothing collapses, the editor survives.
    fireEvent.click(screen.getByRole("button", { name: "collapse all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(true);
    expect(section(1).querySelector("textarea")).not.toBeNull();

    // Accepted: everything collapses and the editor is unmounted with it.
    fireEvent.click(screen.getByRole("button", { name: "collapse all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(false);
    expect(document.querySelector("textarea")).toBeNull();
    expect(confirm).toHaveBeenCalledTimes(2);
  });
});
