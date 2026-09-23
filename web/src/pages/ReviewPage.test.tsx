// Review page collapse behavior, rendered against the mock fixtures
// (VITE_MOCK is set by the vitest config). Change 11 at ?against=base is
// the full r1 diff: /COMMIT_MSG, src/auth/rotate.rs, src/auth/store.rs,
// tests/rotation.rs — i.e. file-0 .. file-3.

import { cleanup, fireEvent, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { Route, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FILE_TREE_TAG_NAME } from "@pierre/trees";
import { COMMIT_MSG_PATH } from "../api/types";
import { renderPage } from "../test/page";
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

const renderReview = (url = "/changes/11?against=base", outside?: ReactNode) =>
  renderPage(
    url,
    <Route path="/changes/:id" element={<ReviewPage />} />,
    outside,
  );

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

/** The head of a diff-range picker, which names the chosen end. */
const picker = (label: string): HTMLButtonElement =>
  screen.getByLabelText<HTMLButtonElement>(label);

/** Opens `label`'s list and chooses the option named `option`. */
const choose = (label: string, option: string) => {
  fireEvent.click(picker(label));
  fireEvent.click(screen.getByRole("option", { name: option }));
};

/** The options `label` offers, each as it reads on screen. */
const optionsOf = (label: string) => {
  fireEvent.click(picker(label));
  const named = screen.getAllByRole("option").map((o) => o.textContent);
  fireEvent.click(picker(label));
  return named;
};

const queryPath = (path: string) =>
  document.querySelector<HTMLElement>(`section[data-diff-path="${path}"]`);
const byPath = (path: string): HTMLElement =>
  must(queryPath(path), `section for ${path}`);

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
    await renderReview();

    expect(isExpanded(section(0))).toBe(true); // /COMMIT_MSG
    expect(section(0).querySelector(".diff-grid")).not.toBeNull();
    for (const i of [1, 2, 3]) {
      expect(isExpanded(section(i))).toBe(false);
      // aria matches reality: no diff body rendered while collapsed.
      expect(section(i).querySelector(".diff-grid")).toBeNull();
    }
  });

  it("toggles a section from its header without scrolling", async () => {
    const { settle } = await renderReview();

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
    await settle();
    expect(railRow("src/auth/rotate.rs")).not.toBeNull();
    expect(railActive()).toBeNull();
  });

  it("rail click expands the target and scrolls only after the expansion is committed", async () => {
    const { settle } = await renderReview();

    // layout-shift case: expanded content sits above the collapsed target.
    expect(isExpanded(section(0))).toBe(true);
    expect(isExpanded(section(1))).toBe(false);
    expect(isExpanded(section(2))).toBe(false);

    fireEvent.click(must(railRow("src/auth/store.rs"), "rail row"));

    expect(scrollCalls).toEqual([{ id: "file-2", expandedAtCall: true }]);
    // …and the rail follows the reveal once the tree repaints.
    await settle();
    expect(railActive()).toBe("src/auth/store.rs");
    expect(isExpanded(section(2))).toBe(true);
    expect(isExpanded(section(1))).toBe(false);
  });

  it("the ] key reveals the next file like a rail click", async () => {
    const { settle } = await renderReview();

    fireEvent.keyDown(window, { key: "]" }); // already expanded (not the regression-guard case)
    fireEvent.keyDown(window, { key: "]" }); // was collapsed — this is the regression-guard case

    expect(scrollCalls).toEqual([
      { id: "file-0", expandedAtCall: true },
      { id: "file-1", expandedAtCall: true },
    ]);
    await settle();
    expect(railActive()).toBe("src/auth/rotate.rs");
    expect(isExpanded(section(1))).toBe(true);
  });

  it("expand all / collapse all flips every section", async () => {
    const { settle } = await renderReview();

    fireEvent.click(screen.getByRole("button", { name: "expand all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "collapse all" }));
    for (const i of [0, 1, 2, 3]) expect(isExpanded(section(i))).toBe(false);
    expect(scrollCalls).toEqual([]);
    await settle();
    expect(railRow("src/auth/rotate.rs")).not.toBeNull();
    expect(railActive()).toBeNull();
  });
});

describe("expansion across diff-range navigation", () => {
  /** Renders change 11 at r1 vs base and expands rotate.rs. */
  async function expandRotate() {
    const page = await renderReview();
    fireEvent.click(
      must(
        byPath("src/auth/rotate.rs").querySelector(".file-header"),
        ".file-header",
      ),
    );
    expect(isExpanded(byPath("src/auth/rotate.rs"))).toBe(true);
    return page;
  }

  /** The additions count in a file's header, which tells the ranges apart. */
  const added = (path: string) =>
    byPath(path).querySelector(".file-header .plus")?.textContent;

  it("keeps expanded files expanded when the base or revision changes", async () => {
    const { settle } = await expandRotate();
    expect(added("src/auth/rotate.rs")).toBe("+19");

    // r0 → r1 interdiff: the same files in the same (tree) order.
    choose("Diff base", "r0 6 comments");
    await settle();
    expect(added("src/auth/rotate.rs")).toBe("+17");
    expect(isExpanded(byPath("src/auth/rotate.rs"))).toBe(true);
    expect(isExpanded(byPath("src/auth/store.rs"))).toBe(false);

    // r0 vs base (the invalid against=0 snaps back to Base): rotate.rs is
    // still open; store.rs stays collapsed; tests/rotation.rs drops out.
    choose("Revision", "r0 6 comments");
    await settle();
    expect(queryPath("tests/rotation.rs")).toBeNull();
    expect(isExpanded(byPath("src/auth/rotate.rs"))).toBe(true);
    expect(isExpanded(byPath("src/auth/store.rs"))).toBe(false);
  });

  it("navigating to another change resets to the default expansion", async () => {
    const { settle } = await expandRotate();

    fireEvent.keyDown(window, { key: "n" }); // the row above: change 12
    await settle();
    expect(isExpanded(byPath(COMMIT_MSG_PATH))).toBe(true);
    expect(isExpanded(byPath("docs/auth-rotation.md"))).toBe(false);
  });
});

describe("change navigation", () => {
  // The graph lists the chain tip first, so n steps up to the child
  // (change 12) and shift+n back down.
  it("n takes the row above in the tag graph, shift+n the row below", async () => {
    const { settle } = await renderReview();
    /** The changes whose diff is on screen, each told by a file only it
     * touches. */
    const shown = () =>
      (
        [
          [11, "src/auth/store.rs"],
          [12, "docs/auth-rotation.md"],
        ] as const
      ).flatMap(([change, path]) => (queryPath(path) ? [change] : []));
    expect(shown()).toEqual([11]);

    fireEvent.keyDown(window, { key: "n" });
    await settle();
    expect(shown()).toEqual([12]);

    fireEvent.keyDown(window, { key: "N", shiftKey: true });
    await settle();
    expect(shown()).toEqual([11]);

    // CapsLock delivers `N` too, so only the held shift may step back.
    fireEvent.keyDown(window, { key: "N" });
    await settle();
    expect(shown()).toEqual([12]);
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
    await renderReview();
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
    choose("Diff base", "r0 6 comments");
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
    await renderReview();
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
    await renderReview(); // full r1 diff; the counts are range-independent anyway

    // change 11: r0 carries 6 root threads, r1 the 3 drafts on it. Replies
    // ride with their thread and are not counted separately.
    expect(optionsOf("Revision")).toEqual(["r0 6 comments", "r1 3 comments"]);

    // The base picker counts the same way; its extra "Base" option has none.
    expect(optionsOf("Diff base")).toEqual([
      "Base",
      "r0 6 comments",
      "r1 3 comments",
    ]);
  });

  it("honors r0 as an explicit diff base instead of snapping to Base", async () => {
    // r0 is a valid interdiff base — selecting it must stick; an M >= 1 guard
    // would wrongly reject it.
    await renderReview("/changes/11?against=0");
    expect(picker("Diff base").textContent).toBe("r0 6 comments");
  });
});

// A thread anchored to an older revision offers the interdiff from that
// revision to the latest, so the reviewer reads the thread against the
// author's answer to it.
describe("the thread's range button", () => {
  const offers = () =>
    screen.queryAllByRole("button", { name: "Diff against latest" });

  it("switches the diff range to the thread's revision → latest", async () => {
    const { settle } = await renderReview(
      "/changes/11?revision=0&against=base",
    );
    fireEvent.click(
      must(
        byPath("src/auth/rotate.rs").querySelector(".file-header"),
        ".file-header",
      ),
    );

    expect(offers().length).toBeGreaterThan(0);
    fireEvent.click(must(offers()[0], "a range button"));
    await settle();

    expect(picker("Diff base").textContent).toBe("r0 6 comments");
    expect(picker("Revision").textContent).toContain("r1");
    // The r0 → r1 range is on screen now, so nothing is left to offer,
    // though its r0 threads still show.
    expect(
      byPath("src/auth/rotate.rs").querySelector(".thread-actions"),
    ).not.toBeNull();
    expect(offers()).toHaveLength(0);
  });
});

// The page holds the revision it opened with, so `r` is how the reviewer
// follows a revision the author pushes mid-review.
describe("the latest-revision shortcut", () => {
  it("r moves the revision to the latest and keeps the diff base", async () => {
    await renderReview("/changes/11?revision=0&against=base");

    fireEvent.keyDown(window, { key: "r" });

    expect(picker("Revision").textContent).toBe("r1 3 comments");
    expect(picker("Diff base").textContent).toBe("Base");
  });
});

// `s` is the keyboard twin of the Submit button: inert until something is
// drafted, then publishes the listed changes.
describe("the s key submits the listed changes' draft decisions", () => {
  let path = "";
  function LocationProbe() {
    const loc = useLocation();
    path = loc.pathname + loc.hash;
    return null;
  }
  it("is inert with nothing drafted, and publishes once a decision is drafted", async () => {
    const { client, settle } = await renderReview(
      "/changes/20",
      <LocationProbe />,
    );
    expect(queryPath("src/wal.rs")).not.toBeNull();

    // A submit would be pending at once.
    fireEvent.keyDown(window, { key: "s" });
    expect(client.isMutating()).toBe(0);
    expect(path).toBe("/changes/20");

    fireEvent.click(screen.getByRole("button", { name: "Review (a)" }));
    fireEvent.click(screen.getByRole("button", { name: "Comment" }));
    await settle();
    expect(
      screen.getByRole("button", { name: /Submit \(s\) · 1/ }),
    ).toBeTruthy();

    fireEvent.keyDown(window, { key: "s" });
    // The drafted count drains once the invalidated drafts overlay refetches.
    await settle();
    expect(screen.getByRole("button", { name: "Submit (s)" })).toBeTruthy();
    expect(path).toBe("/changes/20");
  });
});

// An open modal owns the keyboard: its dialog is in the top layer, but the
// page's window listener still hears every keystroke.
describe("the page shortcuts while the settings popup is open", () => {
  let path = "";
  function LocationProbe() {
    const loc = useLocation();
    path = loc.pathname;
    return null;
  }

  it("navigates on n, and stops once the popup opens", async () => {
    const { settle } = await renderReview(
      "/changes/11?against=base",
      <LocationProbe />,
    );

    fireEvent.keyDown(window, { key: "n" });
    await settle();
    expect(path).toBe("/changes/12");

    // Change 12 is the chain tip, so only shift+n moves from it.
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.keyDown(window, { key: "N", shiftKey: true });
    expect(path).toBe("/changes/12");
  });
});

// Counting comments pinned to a hidden revision would lie about what's shown.
describe("comment counts in the file headers", () => {
  const fcomments = (i: number): string | null =>
    section(i).querySelector(".fcomments")?.textContent ?? null;

  it("counts only this file's threads visible in the current range", async () => {
    // base → r1: the r0 threads are pinned away, so only the r1 drafts and
    // the r0 threads the server ported to r1 show.
    await renderReview("/changes/11?against=base");

    // rotate.rs (file-1): two drafts on r1 — one new-side, one old-side —
    // and the open r0 selection thread ported to its shifted lines.
    expect(fcomments(1)).toBe("3 comments");
    // tests/rotation.rs (file-3): a single r1 draft.
    expect(fcomments(3)).toBe("1 comment");
    // store.rs (file-2): its file thread has no line, so it shows in every
    // range, and its r0 line thread is ported to the file since r1 rewrote
    // the line.
    expect(fcomments(2)).toBe("2 comments");
    // /COMMIT_MSG (file-0): only r0 line threads — no badge.
    expect(fcomments(0)).toBeNull();
  });

  it("ports the resolved threads too when the settings ask for all", async () => {
    localStorage.setItem("nit.review-settings", '{"ported":"all"}');
    await renderReview("/changes/11?against=base");
    // rotate.rs: the three of the open setting plus the resolved r0 line
    // thread ported to its shifted line.
    expect(fcomments(1)).toBe("4 comments");
  });

  it("follows the range: the r0 → r1 interdiff surfaces the r0 threads", async () => {
    // The left column is r0's own tree, so r0-pinned threads reappear there.
    await renderReview("/changes/11?against=0");

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
    const { settle } = await renderReview();
    fireEvent.click(screen.getByRole("button", { name: "expand all" }));

    // rotate.rs hides a run above its first hunk and another between the two.
    const gaps = () => section(1).querySelectorAll(".hunk-row");
    expect(gaps()).toHaveLength(2);
    // The section's first reveal-all button is the top gap's.
    fireEvent.click(
      must(section(1).querySelector(".expand-all"), ".expand-all"),
    );

    await settle();
    expect(gaps()).toHaveLength(1);
    // Every hidden line came in, starting at the file's first.
    expect(section(1).textContent).toContain("unchanged line 1");
  });
});

// A file thread is correctly placed, unlike a thread whose line the shown
// hunks do not cover. The two must not share a group, or a file comment
// reads as a stale one.
describe("file-anchored threads", () => {
  it("renders in the file's own discussion group, with no line excerpt", async () => {
    await renderReview();
    const store = byPath("src/auth/store.rs");
    toggleSection(2);

    const group = must(
      store.querySelector('[data-threads="file"]'),
      "file thread group on store.rs",
    );
    expect(group.textContent).toContain("File discussion");
    expect(group.textContent).toContain(
      "Split the queries into store/tokens.rs",
    );
    expect(group.querySelector(".line-excerpt")).toBeNull();
    expect(store.querySelector('[data-threads="displaced"]')).toBeNull();
  });
});
