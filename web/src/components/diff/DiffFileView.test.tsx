// Diff markup, rendered with a minimal ReviewContext so the assertions are
// about the markup alone: rebase-drift lines render contained — the
// .drift class lands on the changed
// line's gutter and code cell so the CSS can tint them, while the real edit
// beside them stays untagged — and a gap separator carries the reveal
// buttons its size warrants.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { msgFile } from "../../api/fixtures/builders";
import type { DiffFile } from "../../api/types";
import { EXPAND_STEP } from "../../lib/useHunkExpansion";
import { ReviewContext, type ReviewCtx } from "../../pages/reviewContext";
import DiffFileView from "./DiffFileView";

afterEach(cleanup);

const ctx: ReviewCtx = {
  changeNumber: 1,
  selected: 2,
  against: 1,
  latestRevision: 2,
  showRange: () => undefined,
  editingTarget: null,
  setEditingTarget: () => false,
  setEditorDirty: () => undefined,
};

/** Two hunks with a hidden run of `gap` unchanged lines above the first and
 * another between them — a top separator with no hunk above it, then an
 * interior one — and the file ending at the second hunk, so there is no
 * separator below. */
const gapped = (gap: number): DiffFile => ({
  path: "src/gap.rs",
  status: "modified",
  binary: false,
  additions: 1,
  deletions: 0,
  old_total: 4 + 2 * gap,
  new_total: 5 + 2 * gap,
  hunks: [
    {
      old_start: 1 + gap,
      old_lines: 2,
      new_start: 1 + gap,
      new_lines: 3,
      header: "",
      lines: [
        { kind: "context", old: 1 + gap, new: 1 + gap, text: "fn a() {" },
        { kind: "add", new: 2 + gap, text: "    added();" },
        { kind: "context", old: 2 + gap, new: 3 + gap, text: "}" },
      ],
    },
    {
      old_start: 3 + 2 * gap,
      old_lines: 2,
      new_start: 4 + 2 * gap,
      new_lines: 2,
      header: "",
      lines: [
        {
          kind: "context",
          old: 3 + 2 * gap,
          new: 4 + 2 * gap,
          text: "fn b() {",
        },
        { kind: "context", old: 4 + 2 * gap, new: 5 + 2 * gap, text: "}" },
      ],
    },
  ],
});

// A mixed hunk: a real edit (line 1) and a drift edit (line 2) tagged.
const mixed: DiffFile = {
  path: "src/base.rs",
  status: "modified",
  binary: false,
  additions: 1,
  deletions: 1,
  old_total: 4,
  new_total: 4,
  hunks: [
    {
      old_start: 1,
      old_lines: 4,
      new_start: 1,
      new_lines: 4,
      header: "",
      lines: [
        { kind: "del", old: 1, text: "let x = real_old();" },
        { kind: "add", new: 1, text: "let x = real_new();" },
        { kind: "del", old: 2, text: "fn moved(a: A) {", drift: true },
        { kind: "add", new: 2, text: "fn moved(arg: A) {", drift: true },
        { kind: "context", old: 3, new: 3, text: "    body();" },
        { kind: "context", old: 4, new: 4, text: "}" },
      ],
    },
  ],
};

const tree = (layout: "unified" | "split", file: DiffFile = mixed) => (
  <QueryClientProvider client={new QueryClient()}>
    <ReviewContext.Provider value={ctx}>
      <DiffFileView
        file={file}
        layout={layout}
        threads={[]}
        domId="file-0"
        collapsed={false}
        onToggle={() => undefined}
      />
    </ReviewContext.Provider>
  </QueryClientProvider>
);

const renderFile = (layout: "unified" | "split", file?: DiffFile) =>
  render(tree(layout, file));

describe("rebase drift rendering", () => {
  it("tags only the drift line's code cells in unified layout", () => {
    const { container } = renderFile("unified");
    expect(container.querySelectorAll(".code.drift").length).toBe(2);
    const realChanges = container.querySelectorAll(
      ".code.del:not(.drift), .code.add:not(.drift)",
    );
    expect(realChanges.length).toBe(2);
  });

  it("tags the drift gutter and code in split layout", () => {
    const { container } = renderFile("split");
    expect(container.querySelector(".code.drift")).not.toBeNull();
    expect(container.querySelector(".g.drift")).not.toBeNull();
  });
});

// The reviewer's DOM selection is the input `c` reads (lib/selection) and
// it lives in these text nodes, so replacing them mid-selection makes the
// browser remap the range out to whole lines.
describe("code cell text nodes", () => {
  it("survive a re-render", () => {
    const { container, rerender } = renderFile("unified");
    const nodes = () =>
      [...container.querySelectorAll(".code-text")].flatMap((c) => [
        ...c.childNodes,
      ]);
    const before = nodes();
    expect(before.length).toBeGreaterThan(0);

    rerender(tree("unified"));

    // Identity, node by node — an equal-looking replacement is the bug.
    const after = nodes();
    expect(after).toHaveLength(before.length);
    after.forEach((node, i) => {
      expect(node).toBe(before[i]);
    });
  });
});

/** Each separator's buttons, as [class suffix, label] pairs. */
const buttons = (container: HTMLElement) =>
  [...container.querySelectorAll(".hunk-row")].map((row) =>
    [...row.querySelectorAll("button")].map((b) => [
      b.className.replace("hunk-expand ", ""),
      b.textContent,
    ]),
  );

describe("context-expand buttons", () => {
  it("offers the whole gap beside the stepped reveals", () => {
    expect(buttons(renderFile("unified", gapped(25)).container)).toEqual([
      // The top gap has no hunk above to step down from.
      [
        ["expand-all", "+25"],
        ["expand-up", "+10"],
      ],
      [
        ["expand-all", "+25"],
        ["expand-down", "+10"],
        ["expand-up", "+10"],
      ],
    ]);
  });

  it("drops the stepped reveals once one step covers the gap", () => {
    expect(
      buttons(renderFile("unified", gapped(EXPAND_STEP)).container),
    ).toEqual([[["expand-all", "+10"]], [["expand-all", "+10"]]]);
  });
});

describe("a deleted file's outline", () => {
  /** The outline kept line 1 and line 27, so the gap between them hides
   * the 25 lines the collapse took. The delete leaves the new side empty,
   * which is why every hunk sits at new line 0. */
  const deleted: DiffFile = {
    path: "src/gone.rs",
    status: "deleted",
    binary: false,
    additions: 0,
    deletions: 2,
    old_total: 27,
    new_total: 0,
    hunks: [
      {
        old_start: 1,
        old_lines: 1,
        new_start: 0,
        new_lines: 0,
        header: "",
        lines: [{ kind: "del", old: 1, text: "fn a() {" }],
      },
      {
        old_start: 27,
        old_lines: 1,
        new_start: 0,
        new_lines: 0,
        header: "",
        lines: [{ kind: "del", old: 27, text: "fn b() {" }],
      },
    ],
  };

  const interior = [
    ["expand-all", "+25"],
    ["expand-down", "+10"],
    ["expand-up", "+10"],
  ];

  it("offers to reveal the run the collapse hid", () => {
    expect(buttons(renderFile("unified", deleted).container)).toEqual([
      interior,
    ]);
  });

  it("offers the run below the last hunk, which only old_total bounds", () => {
    const tail = { ...deleted, old_total: 40 };
    expect(buttons(renderFile("unified", tail).container)).toEqual([
      interior,
      [
        ["expand-all", "+13"],
        ["expand-down", "+10"],
      ],
    ]);
  });
});

describe("a file the change only renamed", () => {
  const clean: DiffFile = {
    path: "src/renamed.rs",
    old_path: "src/original.rs",
    status: "renamed",
    binary: false,
    additions: 0,
    deletions: 0,
    old_total: 8,
    new_total: 8,
    hunks: [],
  };

  it("stays listed and says why it shows nothing", () => {
    const { container } = renderFile("unified", clean);
    expect(container.querySelector(".file-section")).not.toBeNull();
    expect(container.textContent).toContain("No changed lines to show");
  });
});

describe("a file with lines on one side only", () => {
  it("takes the whole width under a split layout", () => {
    const { container } = renderFile("split", msgFile("auth: rotate tokens"));
    expect(container.querySelector(".diff-grid-unified")).not.toBeNull();
    expect(container.querySelector(".code.half")).toBeNull();
  });

  it("leaves a file with both sides split", () => {
    const { container } = renderFile("split");
    expect(container.querySelector(".diff-grid-split")).not.toBeNull();
  });
});
