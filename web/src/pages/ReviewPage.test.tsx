// Review page collapse behavior, rendered against the mock fixtures
// (VITE_MOCK is set by the vitest config). Change 11 at ?against=base is
// the full r1 diff: /COMMIT_MSG, src/auth/rotate.rs, src/auth/store.rs,
// tests/rotation.rs — i.e. file-0 .. file-3.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FILE_TREE_TAG_NAME } from "@pierre/trees";
import { COMMIT_MSG_PATH } from "../api/types";
import ReviewPage from "./ReviewPage";

// No vitest globals → testing-library cannot auto-cleanup; without this,
// earlier renders stay mounted and their window keydown listeners (and
// duplicate file-N ids) bleed into later tests.
afterEach(cleanup);

/** Every scrollIntoView call on a file section: which one, and whether its
 * diff body was already in the DOM when the call happened. The latter is
 * the regression guard for the collapse pitfall — a scroll issued before
 * the expansion commit would see (and target) the pre-reflow layout. The
 * rail scrolls its own port by scrollTop, so it never lands here. */
let scrollCalls: { id: string; expandedAtCall: boolean }[];

beforeEach(() => {
  // jsdom lays nothing out, so the spy's rAF sample reads every section at
  // rect 0 and can only answer "the last file", overwriting the reveal under
  // test. In a browser that is self-correcting — the sample is taken at the
  // scroll target — but the scrollIntoView double below never scrolls, so
  // here it just wins. No layout, no scroll to spy on: drop the frame.
  window.requestAnimationFrame = () => 0;
  scrollCalls = [];
  Element.prototype.scrollIntoView = function (this: Element) {
    scrollCalls.push({
      id: this.id,
      expandedAtCall: this.querySelector(".diff-grid") !== null,
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

function must<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`expected ${what}`);
  return value;
}

const section = (i: number): HTMLElement =>
  must(document.getElementById(`file-${i}`), `#file-${i}`);
const isExpanded = (el: HTMLElement): boolean =>
  el.querySelector(".file-header")?.getAttribute("aria-expanded") === "true";
/** Clicks a section's header — the toggle every expansion test drives. */
const toggleSection = (i: number) =>
  fireEvent.click(
    must(section(i).querySelector(".file-header"), ".file-header"),
  );

const select = (range: Range) => {
  const sel = must(window.getSelection(), "selection");
  sel.removeAllRanges();
  sel.addRange(range);
};

const queryPath = (path: string) =>
  document.querySelector<HTMLElement>(`section[data-diff-path="${path}"]`);
const byPath = (path: string): HTMLElement =>
  must(queryPath(path), `section for ${path}`);

/** Awaits diff load, signaled by a file's own section — not by the rail,
 * whose tree paints on its own schedule. */
const diffLoaded = (path: string): Promise<HTMLElement> =>
  waitFor(() => byPath(path));

/** The rail renders into a shadow root, out of reach of `screen`. */
const railQuery = (selector: string): Element | null =>
  document
    .querySelector(FILE_TREE_TAG_NAME)
    ?.shadowRoot?.querySelector(selector) ?? null;

const railRow = (path: string): Element | null =>
  railQuery(`[data-item-path="${path}"]`);

/** The path of the rail's active row — the tree's selection. */
const railActive = (): string | null =>
  railQuery('[aria-selected="true"]')?.getAttribute("data-item-path") ?? null;

describe("collapsed-by-default file sections", () => {
  it("starts with every file collapsed except the commit message", async () => {
    renderReview();
    await diffLoaded("src/auth/store.rs");

    expect(isExpanded(section(0))).toBe(true); // /COMMIT_MSG
    expect(section(0).querySelector(".diff-grid")).not.toBeNull();
    for (const i of [1, 2, 3]) {
      expect(isExpanded(section(i))).toBe(false);
      // aria matches reality: no diff body rendered while collapsed.
      expect(section(i).querySelector(".diff-grid")).toBeNull();
    }
  });

  it("toggles a section from its header without scrolling", async () => {
    renderReview();
    await diffLoaded("src/auth/store.rs");

    const header = must(
      section(1).querySelector(".file-header"),
      ".file-header",
    );
    fireEvent.click(header);
    expect(isExpanded(section(1))).toBe(true);
    fireEvent.click(header);
    expect(isExpanded(section(1))).toBe(false);
    // The active file never moved.
    expect(scrollCalls).toEqual([]);
    expect(railActive()).toBeNull();
  });

  it("rail click expands the target and scrolls only after the expansion is committed", async () => {
    renderReview();
    await diffLoaded("src/auth/store.rs");

    // layout-shift case: expanded content sits above the collapsed target.
    expect(isExpanded(section(0))).toBe(true);
    expect(isExpanded(section(1))).toBe(false);
    expect(isExpanded(section(2))).toBe(false);

    fireEvent.click(must(railRow("src/auth/store.rs"), "rail row"));

    expect(scrollCalls).toEqual([{ id: "file-2", expandedAtCall: true }]);
    // …and the rail follows the reveal (the tree repaints off-cycle).
    await waitFor(() => {
      expect(railActive()).toBe("src/auth/store.rs");
    });
    expect(isExpanded(section(2))).toBe(true);
    expect(isExpanded(section(1))).toBe(false);
  });

  it("the ] key reveals the next file like a rail click", async () => {
    renderReview();
    await diffLoaded("src/auth/store.rs");

    fireEvent.keyDown(window, { key: "]" }); // already expanded (not the regression-guard case)
    fireEvent.keyDown(window, { key: "]" }); // was collapsed — this is the regression-guard case

    expect(scrollCalls).toEqual([
      { id: "file-0", expandedAtCall: true },
      { id: "file-1", expandedAtCall: true },
    ]);
    await waitFor(() => {
      expect(railActive()).toBe("src/auth/rotate.rs");
    });
    expect(isExpanded(section(1))).toBe(true);
  });

  it("expand all / collapse all flips every section", async () => {
    renderReview();
    await diffLoaded("src/auth/store.rs");

    fireEvent.click(screen.getByRole("button", { name: "expand all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "collapse all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(false);
    expect(scrollCalls).toEqual([]);
    expect(railActive()).toBeNull();
  });
});

describe("expansion across diff-range navigation", () => {
  /** Renders change 11 at r1 vs base and expands rotate.rs. */
  async function expandRotate() {
    renderReview();
    await diffLoaded("src/auth/store.rs");
    fireEvent.click(
      must(
        byPath("src/auth/rotate.rs").querySelector(".file-header"),
        ".file-header",
      ),
    );
    expect(isExpanded(byPath("src/auth/rotate.rs"))).toBe(true);
  }

  it("keeps expanded files expanded when the base or revision changes", async () => {
    await expandRotate();

    // r0 → r1 interdiff: the same files in the same (tree) order, so the
    // settled range is the signal that the new diff has rendered.
    fireEvent.change(screen.getByLabelText("Diff base"), {
      target: { value: "0" },
    });
    await waitFor(() => {
      expect(document.querySelector('[data-diff-ready="0"]')).not.toBeNull();
    });
    expect(isExpanded(byPath("src/auth/rotate.rs"))).toBe(true);
    expect(isExpanded(byPath("src/auth/store.rs"))).toBe(false);

    // r0 vs base (the invalid against=0 snaps back to Base): rotate.rs is
    // still open; store.rs stays collapsed; tests/rotation.rs drops out.
    fireEvent.change(screen.getByLabelText("Revision"), {
      target: { value: "0" },
    });
    await waitFor(() => {
      // Every section is absent during the refetch gap, so the removal
      // alone would pass before the new diff renders.
      expect(queryPath("src/auth/rotate.rs")).not.toBeNull();
      expect(queryPath("tests/rotation.rs")).toBeNull();
    });
    expect(isExpanded(byPath("src/auth/rotate.rs"))).toBe(true);
    expect(isExpanded(byPath("src/auth/store.rs"))).toBe(false);
  });

  it("navigating to another change resets to the default expansion", async () => {
    await expandRotate();

    fireEvent.keyDown(window, { key: "n" }); // next change in the chain
    await diffLoaded("docs/auth-rotation.md");
    expect(isExpanded(byPath(COMMIT_MSG_PATH))).toBe(true);
    expect(isExpanded(byPath("docs/auth-rotation.md"))).toBe(false);
  });
});

// `c` on a selection that no comment range can express explains itself in a
// bubble by the selection. The reviewer's next move is to select again, so
// the bubble outlives the keystroke and dies with the selection it answers.
describe("the selection-miss bubble", () => {
  // jsdom has no layout, and no Range.getBoundingClientRect at all — the
  // bubble's anchor asks the rejected range where it sits.
  beforeEach(() => {
    Range.prototype.getBoundingClientRect = () => new DOMRect();
  });

  const bubble = () => document.querySelector(".selection-miss");

  /** Selects the del/add pair rotate.rs rewrites at line 20 — one line per
   * side, so no side can express the range (lib/selection's mixed-sides). */
  async function selectAcrossSides() {
    renderReview();
    await diffLoaded("src/auth/rotate.rs");
    toggleSection(1);
    const codeText = (kind: string) => {
      const cell = [...section(1).querySelectorAll(".code")].find((c) =>
        c.classList.contains(kind),
      );
      return must(must(cell, `a ${kind} cell`).querySelector(".code-text"), "");
    };
    const range = document.createRange();
    range.setStart(codeText("del"), 0);
    range.setEndAfter(codeText("add"));
    select(range);
  }

  it("names the rule, and stays until the next selection", async () => {
    await selectAcrossSides();

    fireEvent.keyDown(window, { key: "c" });
    expect(bubble()?.textContent).toContain("one side of the diff");
    // No editor: the press drafted nothing.
    expect(document.querySelector("textarea")).toBeNull();

    // Idle keystrokes leave it be, and so does the selectionchange the
    // rejected selection itself queues — only a new one retires it.
    fireEvent.keyDown(window, { key: "]" });
    fireEvent(document, new Event("selectionchange"));
    expect(bubble()).not.toBeNull();

    fireEvent(document, new Event("selectstart"));
    expect(bubble()).toBeNull();
  });

  it("goes with the diff it was measured against", async () => {
    await selectAcrossSides();
    fireEvent.keyDown(window, { key: "c" });
    expect(bubble()).not.toBeNull();

    // Switching the range leaves different lines where it hangs.
    fireEvent.change(screen.getByLabelText("Diff base"), {
      target: { value: "0" },
    });
    expect(bubble()).toBeNull();
  });
});

// Collapsing the section that hosts the open inline editor unmounts it,
// which is a discard path: it must route through confirmDiscard (i.e.
// window.confirm while dirty) like every other editor teardown.
describe("collapse with an open dirty comment editor", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Leaves a dirty draft open on section(1); needs a manual caret + 'c'
   * because clicking a line doesn't open an editor (see lib/selection). */
  async function openDirtyEditor() {
    renderReview();
    await diffLoaded("src/auth/store.rs");
    toggleSection(1);
    const code = must(section(1).querySelector(".code-text"), ".code-text");
    const range = document.createRange();
    range.selectNodeContents(code);
    range.collapse(true);
    select(range);
    fireEvent.keyDown(window, { key: "c" });
    fireEvent.change(must(section(1).querySelector("textarea"), "textarea"), {
      target: { value: "half-typed nit" },
    });
  }

  it("declined header collapse keeps the file expanded and the editor mounted", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    await openDirtyEditor();

    toggleSection(1);

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(isExpanded(section(1))).toBe(true);
    expect(section(1).querySelector("textarea")).not.toBeNull();
  });

  it("accepted header collapse discards the draft and collapses the section", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    await openDirtyEditor();

    toggleSection(1);

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(isExpanded(section(1))).toBe(false);
    // Re-expanding must not resurrect an empty editor at the stale anchor.
    toggleSection(1);
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

// Each revision option is tagged with its own comment-thread count, so the
// reviewer sees where discussion sits before switching the diff range.
describe("comment counts in the diff-range dropdowns", () => {
  it("tags each revision option with its thread count", async () => {
    renderReview(); // full r1 diff; the counts are range-independent anyway
    await diffLoaded("src/auth/store.rs");

    // change 11: r0 carries 5 root threads, r1 the 3 drafts on it. Replies
    // ride with their thread and are not counted separately.
    const revSelect = screen.getByLabelText<HTMLSelectElement>("Revision");
    expect(Array.from(revSelect.options).map((o) => o.textContent)).toEqual([
      "r0 · 5 comments",
      "r1 · 3 comments",
    ]);

    // The base picker counts the same way; its extra "Base" option has none.
    const baseSelect = screen.getByLabelText<HTMLSelectElement>("Diff base");
    expect(Array.from(baseSelect.options).map((o) => o.textContent)).toEqual([
      "Base",
      "r0 · 5 comments",
      "r1 · 3 comments",
    ]);
  });

  it("honors r0 as an explicit diff base instead of snapping to Base", async () => {
    // r0 is a valid interdiff base — selecting it must stick; an M >= 1 guard
    // would wrongly reject it.
    renderReview("/changes/11?against=0");
    await diffLoaded("src/auth/store.rs");
    const baseSelect = screen.getByLabelText<HTMLSelectElement>("Diff base");
    expect(baseSelect.value).toBe("0");
  });
});

// `s` is the keyboard twin of the Submit button: inert until something is
// staged, then publishes the chain.
describe("the s key submits the chain's staged decisions", () => {
  // jsdom has no top-layer, so the review modal's showModal() is absent — stub
  // it so opening the modal to stage a decision doesn't throw.
  beforeEach(() => {
    HTMLDialogElement.prototype.showModal = function () {
      this.open = true;
    };
  });

  let path = "";
  function LocationProbe() {
    const loc = useLocation();
    path = loc.pathname + loc.hash;
    return null;
  }
  function renderChange20() {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    return render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/changes/20"]}>
          <LocationProbe />
          <Routes>
            <Route path="/changes/:id" element={<ReviewPage />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    );
  }

  it("is inert with nothing staged, and publishes once a decision is staged", async () => {
    renderChange20();
    await diffLoaded("src/wal.rs");

    fireEvent.keyDown(window, { key: "s" });
    expect(path).toBe("/changes/20");

    fireEvent.click(screen.getByRole("button", { name: "Review (a)" }));
    fireEvent.click(screen.getByRole("button", { name: "Comment" }));
    await screen.findByRole("button", { name: /Submit chain \(s\) · 1/ });

    fireEvent.keyDown(window, { key: "s" });
    // The staged count drains once the invalidated drafts overlay refetches.
    await screen.findByRole("button", { name: "Submit chain (s)" });
    expect(path).toBe("/changes/20");
  });
});

// Counting comments pinned to a hidden revision would lie about what's shown.
describe("comment counts in the file headers", () => {
  const fcomments = (i: number): string | null =>
    section(i).querySelector(".fcomments")?.textContent ?? null;

  it("counts only this file's threads visible in the current range", async () => {
    // base → r1: the r0 threads are pinned away, so only the r1 drafts show.
    renderReview("/changes/11?against=base");
    await diffLoaded("src/auth/store.rs");

    // rotate.rs (file-1): two drafts on r1 — one new-side, one old-side.
    expect(fcomments(1)).toBe("2 comments");
    // tests/rotation.rs (file-3): a single r1 draft.
    expect(fcomments(3)).toBe("1 comment");
    // store.rs (file-2) and /COMMIT_MSG (file-0): only r0 threads, all
    // pinned to a revision this range does not show — no badge.
    expect(fcomments(2)).toBeNull();
    expect(fcomments(0)).toBeNull();
  });

  it("follows the range: the r0 → r1 interdiff surfaces the r0 threads", async () => {
    // The left column is r0's own tree, so r0-pinned threads reappear there.
    renderReview("/changes/11?against=0");
    await diffLoaded("src/auth/rotate.rs");

    // rotate.rs: three r0 threads (lines 21/22/23) on the left + one r1
    // draft on the right; the old-side r1 draft has no column here.
    expect(fcomments(1)).toBe("4 comments");
  });
});

// The reveal-all button on a separator folds the gap into a neighbouring
// hunk, leaving the hunks contiguous — so the separator disappears. The top
// gap is the case with no hunk above it: it must fold up into the hunk
// below.
describe("context expansion", () => {
  it("reveals a whole top gap in one click", async () => {
    renderReview();
    await diffLoaded("src/auth/rotate.rs");
    fireEvent.click(screen.getByRole("button", { name: "expand all" }));

    // rotate.rs hides a run above its first hunk and another between the two.
    const gaps = () => section(1).querySelectorAll(".hunk-row");
    expect(gaps()).toHaveLength(2);
    // The section's first reveal-all button is the top gap's.
    fireEvent.click(
      must(section(1).querySelector(".expand-all"), ".expand-all"),
    );

    await waitFor(() => {
      expect(gaps()).toHaveLength(1);
    });
    // Every hidden line came in, starting at the file's first.
    expect(section(1).textContent).toContain("unchanged line 1");
  });
});
