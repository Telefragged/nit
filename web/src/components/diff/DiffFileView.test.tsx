// Diff markup, rendered with a minimal ReviewContext so the assertions are
// about the markup alone.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  add,
  addedFile,
  attachedFile,
  ctx as both,
  del,
  msgFile,
  reviewCtx,
  uiThread,
} from "../../api/fixtures/builders";
import { COMMIT_MSG_PATH, type DiffFile } from "../../api/types";
import type { DiffLink, LinkSpan } from "../../lib/diffLink";
import type {
  FilePlace,
  LineOrRange,
  RenderedAnchor,
  UiThread,
} from "../../lib/comments";
import { EXPAND_STEP, fileLinesQuery } from "../../lib/useHunkExpansion";
import { ReviewContext } from "../../pages/reviewContext";
import DiffFileView from "./DiffFileView";

afterEach(cleanup);

const ctx = reviewCtx({
  selected: 2,
  against: "r1",
  latestRevision: 2,
});

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

const tree = (
  layout: "unified" | "split",
  file: DiffFile = mixed,
  threads: UiThread[] = [],
  client = new QueryClient(),
  value = ctx,
) => (
  <QueryClientProvider client={client}>
    <ReviewContext.Provider value={value}>
      <DiffFileView
        file={file}
        layout={layout}
        threads={threads}
        domId="file-0"
        collapsed={false}
        onToggle={() => undefined}
        reveal={null}
        onRevealed={() => undefined}
      />
    </ReviewContext.Provider>
  </QueryClientProvider>
);

const renderFile = (
  layout: "unified" | "split",
  file?: DiffFile,
  threads?: UiThread[],
) => render(tree(layout, file, threads));

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

/** Each separator's buttons, as [kind, title] pairs. */
const buttons = (container: HTMLElement) =>
  [...container.querySelectorAll(".hunk-row")].map((row) =>
    [...row.querySelectorAll("button")].map((b) => [
      /expand-\w+/.exec(b.className)?.[0],
      b.title,
    ]),
  );

describe("context-expand buttons", () => {
  it("puts the show-all button between the stepped buttons", () => {
    expect(buttons(renderFile("unified", gapped(25)).container)).toEqual([
      // The top gap has no hunk above to step down from.
      [
        ["expand-up", "Show 10 more lines above"],
        ["expand-all", "Show all 25 lines"],
      ],
      [
        ["expand-up", "Show 10 more lines above"],
        ["expand-all", "Show all 25 lines"],
        ["expand-down", "Show 10 more lines below"],
      ],
    ]);
  });

  it("drops the stepped reveals once one step covers the gap", () => {
    expect(
      buttons(renderFile("unified", gapped(EXPAND_STEP)).container),
    ).toEqual([
      [["expand-all", "Show all 10 lines"]],
      [["expand-all", "Show all 10 lines"]],
    ]);
  });

  it("reveal the hidden lines of a commit message like those of a file", () => {
    const message = { ...gapped(EXPAND_STEP), path: COMMIT_MSG_PATH };
    expect(buttons(renderFile("unified", message).container)).toEqual([
      [["expand-all", "Show all 10 lines"]],
      [["expand-all", "Show all 10 lines"]],
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

/** A published thread on `mixed`'s new side at revision `ctx.selected`. */
const threadAt = (id: number, at: LineOrRange): UiThread =>
  uiThread({
    id,
    anchor: { file: { version: "r2", path: "src/base.rs", anchor: at } },
  });

/** Points at the one rendered thread, returning its wrapper. */
const hoverThread = (container: HTMLElement) => {
  const item = container.querySelector(".meta-item");
  if (!item) throw new Error("no thread rendered");
  fireEvent.mouseEnter(item);
  return item;
};

describe("a drag over the line numbers", () => {
  it("links the lines from the pressed number to the one under the pointer", () => {
    const linked: DiffLink[] = [];
    const { container } = render(
      tree("unified", mixed, [], undefined, {
        ...ctx,
        selectLink: (to) => linked.push(to),
      }),
    );
    const cell = (no: number) => {
      const code = container.querySelector(`.code[data-new="${no}"]`);
      if (!code) throw new Error(`no line ${no}`);
      return code;
    };
    const number = cell(1).parentElement?.querySelectorAll(".line-link")[0];
    if (!number) throw new Error("no line number");
    fireEvent.mouseDown(number);
    fireEvent.mouseOver(cell(3));
    expect(cell(2).classList).toContain("linked");
    fireEvent.mouseUp(document);
    expect(linked).toEqual([
      { file: mixed.path, lines: { side: "new", start: 1, end: 3 } },
    ]);
  });
});

describe("hovering a thread", () => {
  it.each(["unified", "split"] as const)(
    "tints its whole line while the pointer is over it (%s)",
    (layout) => {
      const { container } = renderFile(layout, mixed, [
        threadAt(1, { line: 3 }),
      ]);
      expect(container.querySelector(".line-hover")).toBeNull();
      const item = hoverThread(container);
      const code = container.querySelector(".code.line-hover");
      expect(code?.textContent).toContain("body();");
      fireEvent.mouseLeave(item);
      expect(container.querySelector(".line-hover")).toBeNull();
    },
  );

  it("brightens its selection instead of the line", () => {
    const { container } = renderFile("unified", mixed, [
      threadAt(1, {
        range: { start_line: 3, start_char: 4, end_line: 3, end_char: 8 },
      }),
    ]);
    hoverThread(container);
    expect(container.querySelector(".line-hover")).toBeNull();
    expect(container.querySelector(".comment-range-active")?.textContent).toBe(
      "body",
    );
  });
});

describe("a rendered attachment", () => {
  const plan = attachedFile("plan.md", "# Plan\n\nStep one\nand two.\n", 2);
  const threadOn = (at: LineOrRange) =>
    uiThread({
      id: 1,
      anchor: { file: { version: "r2", path: plan.path, anchor: at } },
    });
  /** The source lines of the block that the one thread shows under. */
  const threadBlock = (container: HTMLElement) => {
    const block = container.querySelector(".meta-row")?.previousElementSibling;
    return block instanceof HTMLElement
      ? [block.dataset.start, block.dataset.end]
      : null;
  };

  it("shows a line's thread under the block that holds the line", () => {
    const { container } = renderFile("unified", plan, [threadOn({ line: 4 })]);
    expect(threadBlock(container)).toEqual(["3", "4"]);
    expect(container.querySelector('[data-threads="displaced"]')).toBeNull();
  });

  it("shows a thread on a blank line under the block above", () => {
    const { container } = renderFile("unified", plan, [threadOn({ line: 2 })]);
    expect(threadBlock(container)).toEqual(["1", "1"]);
  });

  it("opens the editor under the block of its draft", () => {
    const at: LineOrRange = {
      range: { start_line: 3, start_char: 0, end_line: 4, end_char: 8 },
    };
    const { container } = render(
      tree("unified", plan, [], undefined, {
        ...ctx,
        editingTarget: { file: plan.path, side: "new", at },
      }),
    );
    const block = container.querySelector('.doc-block[data-end="4"]');
    expect(block?.getAttribute("data-end-char")).toBe("8");
    expect(block?.nextElementSibling?.querySelector("textarea")).not.toBeNull();
  });

  it("tints the innermost part that a hovered thread's anchor covers", () => {
    const table = attachedFile("t.md", "| A | B |\n|---|---|\n| x | y |\n", 2);
    const hovered = (at: LineOrRange) => {
      const thread = uiThread({
        id: 1,
        anchor: { file: { version: "r2", path: table.path, anchor: at } },
      });
      const { container } = renderFile("unified", table, [thread]);
      hoverThread(container);
      const tinted = [...container.querySelectorAll(".line-hover")].map(
        (el) => el.tagName,
      );
      cleanup();
      return tinted;
    };
    const cell = { start_line: 3, start_char: 6, end_line: 3, end_char: 7 };
    expect(hovered({ range: cell })).toEqual(["TD"]);
    expect(hovered({ line: 3 })).toEqual(["TR"]);
  });

  it("tints the block above a hovered thread on a blank line", () => {
    const doc = attachedFile("d.md", "First.\n\nLast one.\n", 2);
    const thread = uiThread({
      id: 1,
      anchor: {
        file: { version: "r2", path: doc.path, anchor: { line: 2 } },
      },
    });
    const { container } = renderFile("unified", doc, [thread]);
    hoverThread(container);
    const tinted = [...container.querySelectorAll(".line-hover")].map((el) =>
      el.textContent.replace(/^\d+/, ""),
    );
    expect(tinted).toEqual(["First."]);
  });

  it("tints a part in each block that a hovered thread's range spans", () => {
    const doc = attachedFile("d.md", "First.\n\n- a\n- b\n\nLast one.\n", 2);
    const thread = uiThread({
      id: 1,
      anchor: {
        file: {
          version: "r2",
          path: doc.path,
          anchor: {
            range: { start_line: 1, start_char: 2, end_line: 6, end_char: 4 },
          },
        },
      },
    });
    const { container } = renderFile("unified", doc, [thread]);
    hoverThread(container);
    const tinted = [...container.querySelectorAll(".line-hover")].map((el) =>
      el.textContent.replace(/^\d+/, ""),
    );
    expect(tinted).toEqual(["First.", "a", "b", "Last one."]);
  });

  it("marks the linked part, or else the block that the link overlaps", () => {
    const table = attachedFile("t.md", "| A | B |\n|---|---|\n| x | y |\n", 2);
    const shown = (lines: LinkSpan["lines"]) => {
      const { container } = render(
        tree("unified", table, [], undefined, {
          ...ctx,
          link: { file: table.path, lines },
        }),
      );
      const marked = [...container.querySelectorAll(".linked")].map(
        (el) => el.tagName,
      );
      cleanup();
      return marked;
    };
    expect(shown({ side: "new", start: 3, end: 3, chars: [6, 7] })).toEqual([
      "TD",
    ]);
    expect(shown({ side: "new", start: 3, end: 3 })).toEqual(["TR"]);
    expect(shown({ side: "new", start: 2, end: 2 })).toEqual(["DIV"]);
  });

  it("links the blocks from the pressed number to the one under the pointer", () => {
    const linked: DiffLink[] = [];
    const { container } = render(
      tree("unified", plan, [], undefined, {
        ...ctx,
        selectLink: (to) => linked.push(to),
      }),
    );
    const [title, steps] = container.querySelectorAll(".doc-block");
    const number = title?.querySelector(".line-link");
    if (!number || !steps) throw new Error("no blocks");
    fireEvent.mouseDown(number);
    fireEvent.mouseOver(steps);
    expect(steps.classList).toContain("linked");
    fireEvent.mouseUp(document);
    expect(linked).toEqual([
      { file: plan.path, lines: { side: "new", start: 1, end: 4 } },
    ]);
  });

  it("opens the editor under the block that contains the part of its draft", () => {
    const list = attachedFile("l.md", "- a\n  - b\n  - c\n", 2);
    const { container } = render(
      tree("unified", list, [], undefined, {
        ...ctx,
        editingTarget: {
          file: list.path,
          side: "new",
          at: { line: 2 },
        },
      }),
    );
    const block = container.querySelector('.doc-block[data-end="3"]');
    expect(block?.nextElementSibling?.querySelector("textarea")).not.toBeNull();
  });

  it("renders from its own lines, without a read", () => {
    const file = attachedFile("plan.md", "# Plan", 2);
    const { container } = renderFile("unified", file);
    expect(container.querySelector(".rendered-markdown h1")?.textContent).toBe(
      "Plan",
    );
  });
});

describe("a rendered Markdown diff", () => {
  const paragraphs = ["# Title", "One.", "Two.", "Three."];
  const lines = [
    ...paragraphs.flatMap((text, i) => [
      both(2 * i + 1, 2 * i + 1, text),
      both(2 * i + 2, 2 * i + 2, ""),
    ]),
    del(9, "old end."),
    add(9, "new end."),
  ];
  const doc: DiffFile = {
    path: "docs/long.md",
    status: "modified",
    binary: false,
    additions: 1,
    deletions: 1,
    old_total: 9,
    new_total: 9,
    hunks: [
      {
        old_start: 1,
        old_lines: 9,
        new_start: 1,
        new_lines: 9,
        header: "",
        lines,
      },
    ],
  };
  const shown = (container: HTMLElement) =>
    [...container.querySelectorAll(".doc-block")].map((b) => b.textContent);

  it("hides the unchanged blocks far from a change until the reviewer reveals them", () => {
    const { container } = renderFile("unified", doc);
    fireEvent.click(screen.getByRole("button", { name: "Rendered" }));
    expect(shown(container)).toEqual(["7Three.", "9old end.", "9new end."]);
    fireEvent.click(screen.getByTitle("Show 1 more block above"));
    expect(shown(container)).toEqual([
      "5Two.",
      "7Three.",
      "9old end.",
      "9new end.",
    ]);
    fireEvent.click(screen.getByTitle("Show all 2 blocks"));
    expect(shown(container)).toHaveLength(6);
  });

  it("reveals the hidden blocks under a change one at a time", () => {
    const below: DiffFile = {
      ...doc,
      old_total: 8,
      new_total: 8,
      hunks: [
        {
          old_start: 1,
          old_lines: 8,
          new_start: 1,
          new_lines: 8,
          header: "",
          lines: [
            del(1, "old start."),
            add(1, "new start."),
            ...lines.slice(1, 8),
          ],
        },
      ],
    };
    const { container } = renderFile("unified", below);
    fireEvent.click(screen.getByRole("button", { name: "Rendered" }));
    expect(shown(container)).toEqual(["1old start.", "1new start.", "3One."]);
    fireEvent.click(screen.getByTitle("Show 1 more block below"));
    expect(shown(container)).toEqual([
      "1old start.",
      "1new start.",
      "3One.",
      "5Two.",
    ]);
  });

  it("shows a block with a thread", () => {
    const thread = uiThread({
      id: 1,
      anchor: {
        file: { version: "r2", path: doc.path, anchor: { line: 3 } },
      },
    });
    const { container } = renderFile("unified", doc, [thread]);
    fireEvent.click(screen.getByRole("button", { name: "Rendered" }));
    expect(shown(container)).toEqual([
      "3One.",
      "7Three.",
      "9old end.",
      "9new end.",
    ]);
  });
});

describe("a Markdown file", () => {
  const doc: DiffFile = {
    path: "docs/a.md",
    status: "modified",
    binary: false,
    additions: 1,
    deletions: 1,
    old_total: 3,
    new_total: 3,
    hunks: [
      {
        old_start: 3,
        old_lines: 1,
        new_start: 3,
        new_lines: 1,
        header: "",
        lines: [del(3, "old *text*"), add(3, "new *text*")],
      },
    ],
  };

  it("renders the whole file, with the changed blocks marked", () => {
    const client = new QueryClient();
    client.setQueryData(fileLinesQuery(ctx, doc).queryKey, {
      lines: [
        both(1, 1, "# Title"),
        both(2, 2, ""),
        ...(doc.hunks[0]?.lines ?? []),
      ],
    });
    const { container } = render(tree("unified", doc, [], client));
    expect(container.querySelector(".diff-grid")).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Rendered" }));
    expect(container.querySelector(".diff-grid")).toBeNull();
    expect(container.querySelector(".rendered-markdown h1")?.textContent).toBe(
      "Title",
    );
    expect(container.querySelector(".doc-removed")?.textContent).toBe(
      "3old text",
    );
    expect(container.querySelector(".doc-added")?.textContent).toBe(
      "3new text",
    );
  });
});

describe("a rendered image", () => {
  const shot = attachedFile("ui.png", "", 2);
  const rect = { x: 10, y: 20, width: 40, height: 30 };
  const region: RenderedAnchor = { region: { rect } };
  const threadOn = (version: "r1" | "r2", at: FilePlace = region) =>
    uiThread({
      id: 1,
      anchor: { file: { version, path: shot.path, anchor: at } },
    });
  const linked: DiffLink = {
    file: shot.path,
    lines: null,
    rendered: { side: "new", at: region },
  };
  const load = (container: HTMLElement) => {
    const img = container.querySelector("img");
    if (!img) throw new Error("no image");
    Object.defineProperty(img, "naturalWidth", { value: 200 });
    Object.defineProperty(img, "naturalHeight", { value: 100 });
    fireEvent.load(img);
    img.getBoundingClientRect = () => new DOMRect(0, 0, 200, 100);
    return img;
  };

  it("outlines the region of a thread, in percent of the image", () => {
    const { container } = renderFile("unified", shot, [threadOn("r2")]);
    expect(container.querySelector(".rendered-outline")).toBeNull();
    load(container);
    const outline = container.querySelector<HTMLElement>(
      ".rendered-outline.outline-open",
    );
    expect(outline?.style.left).toBe("5%");
    expect(outline?.style.top).toBe("20%");
    expect(outline?.style.width).toBe("20%");
    expect(outline?.style.height).toBe("30%");
    expect(outline?.querySelector('[data-thread="1"]')).not.toBeNull();
    expect(container.querySelector(".floating-panel")).toBeNull();
  });

  it("lists a region of another version with the comments not shown", () => {
    const { container } = renderFile("unified", shot, [threadOn("r1")]);
    load(container);
    expect(container.querySelector(".rendered-outline")).toBeNull();
    expect(
      container.querySelector('[data-threads="displaced"] [data-thread="1"]'),
    ).not.toBeNull();
  });

  it("shows the threads of the linked region in the floating panel", () => {
    const { container } = render(
      tree("unified", shot, [threadOn("r2")], undefined, {
        ...ctx,
        link: linked,
      }),
    );
    load(container);
    expect(container.querySelector(".outline-linked")).not.toBeNull();
    const panel = container.querySelector(".floating-panel");
    expect(panel?.textContent).toContain("region 10,20 40x30");
    expect(panel?.querySelector('[data-thread="1"]')).not.toBeNull();
  });

  it("opens the editor of a draft on a region in the floating panel", () => {
    const { container } = render(
      tree("unified", shot, [], undefined, {
        ...ctx,
        editingTarget: { file: shot.path, side: "new", at: region },
      }),
    );
    load(container);
    expect(container.querySelector(".outline-editing")).not.toBeNull();
    expect(container.querySelector(".floating-panel textarea")).not.toBeNull();
  });

  it("unlinks the rectangle on a click without a drag", () => {
    const selectLink = vi.fn();
    const { container } = render(
      tree("unified", shot, [], undefined, {
        ...ctx,
        link: linked,
        selectLink,
      }),
    );
    const img = load(container);
    fireEvent.mouseDown(img, { button: 0, clientX: 50, clientY: 50 });
    fireEvent.mouseUp(document);
    expect(selectLink).toHaveBeenCalledWith({ file: shot.path, lines: null });
  });

  it("unlinks the rectangle when text elsewhere is selected", () => {
    const selectLink = vi.fn();
    render(
      tree("unified", shot, [], undefined, {
        ...ctx,
        link: linked,
        selectLink,
      }),
    );
    const text = document.createElement("p");
    text.textContent = "elsewhere";
    document.body.append(text);
    const range = document.createRange();
    range.selectNodeContents(text);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    fireEvent(document, new Event("selectionchange"));
    expect(selectLink).toHaveBeenCalledWith({ file: shot.path, lines: null });
    document.getSelection()?.removeAllRanges();
    text.remove();
  });

  it("links the rectangle that a drag covers, in the pixels of the image", () => {
    const selectLink = vi.fn();
    const { container } = render(
      tree("unified", shot, [], undefined, { ...ctx, selectLink }),
    );
    const img = load(container);
    fireEvent.mouseDown(img, { button: 0, clientX: 50, clientY: 50 });
    fireEvent.mouseMove(document, { clientX: 10, clientY: 20 });
    fireEvent.mouseUp(document);
    expect(selectLink).toHaveBeenCalledWith({
      file: shot.path,
      lines: null,
      rendered: {
        side: "new",
        at: { region: { rect: { x: 10, y: 20, width: 40, height: 30 } } },
      },
    });
  });
});

describe("the view of a file that renders", () => {
  const page = addedFile("page.html", "<p>x</p>");
  const element: DiffLink = {
    file: page.path,
    lines: null,
    rendered: { side: "new", at: { element: "/html/body/p" } },
  };

  it("is the raw view, until a link specifies an element", () => {
    const { container, rerender } = render(tree("unified", page));
    expect(container.querySelector(".frame-stage")).toBeNull();
    rerender(tree("unified", page, [], undefined, { ...ctx, link: element }));
    expect(container.querySelector(".frame-stage")).not.toBeNull();
  });

  it("unlinks the element when the reviewer chooses the raw view", () => {
    const selectLink = vi.fn();
    render(
      tree("unified", page, [], undefined, {
        ...ctx,
        link: element,
        selectLink,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Raw" }));
    expect(selectLink).toHaveBeenCalledWith({ file: page.path, lines: null });
  });
});
