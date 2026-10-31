// selectionTarget: DOM selections → comment-range draft targets. The DOM
// built here mirrors DiffFileView's contract exactly (section data attr,
// .code cell with data-old/data-new, .sign + .code-text spans). The diff
// is a CSS grid of divs/spans, not a table — jsdom does no layout, so only
// the element structure, classes and data attributes matter here.

import { beforeEach, describe, expect, it } from "vitest";
import {
  cellLine,
  linesRange,
  selectionAnchorSide,
  selectionTarget,
} from "./selection";

interface RowSpec {
  old?: number;
  new?: number;
  text: string;
}

function unifiedCell(r: RowSpec): string {
  const attrs =
    (r.old !== undefined ? ` data-old="${r.old}"` : "") +
    (r.new !== undefined ? ` data-new="${r.new}"` : "");
  return (
    `<div class="line-row"><span class="g">${r.old ?? ""}</span>` +
    `<span class="g">${r.new ?? ""}</span>` +
    `<span class="code"${attrs}><span class="sign">+</span>` +
    `<span class="code-text">${r.text}</span></span></div>`
  );
}

/** The file-section shell both layouts share; `rows` is the pre-joined
 * inner row HTML. */
function mountSection(
  path: string,
  gridClass: string,
  rows: string,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "file-section";
  section.setAttribute("data-diff-path", path);
  section.innerHTML = `<div class="diff-grid ${gridClass}">${rows}</div>`;
  document.body.appendChild(section);
  return section;
}

/** One file section in unified layout; `hunks` are row groups separated by
 * hunk rows, like real diffs. */
function mountUnified(hunks: RowSpec[][], path = "src/a.rs"): HTMLElement {
  return mountSection(
    path,
    "diff-grid-unified",
    hunks
      .map(
        (rows) =>
          `<div class="hunk-row">@@</div>` + rows.map(unifiedCell).join(""),
      )
      .join(""),
  );
}

/** The text node carrying row `i`'s code (document order across hunks). */
function textNode(section: HTMLElement, i: number): Text {
  const cell = section.querySelectorAll(".code-text")[i];
  if (!cell) throw new Error(`no .code-text at row ${i}`);
  return cell.firstChild as Text;
}

function rangeOf(
  start: Node,
  startOffset: number,
  end: Node,
  endOffset: number,
): Range {
  const r = document.createRange();
  r.setStart(start, startOffset);
  r.setEnd(end, endOffset);
  return r;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

const ROWS: RowSpec[] = [
  { old: 10, new: 12, text: "alpha beta gamma" },
  { new: 13, text: "added line one" },
  { new: 14, text: "added line two" },
  { old: 11, text: "deleted line" },
  { old: 12, new: 15, text: "tail line" },
];

describe("selectionTarget, unified layout", () => {
  it("maps a partial single-line selection", () => {
    const s = mountUnified([ROWS]);
    const t = textNode(s, 1);
    expect(selectionTarget(rangeOf(t, 2, t, 7))).toEqual({
      file: "src/a.rs",
      side: "new",
      at: {
        selection: { start_line: 13, start_char: 2, end_line: 13, end_char: 7 },
      },
    });
  });

  it("maps a context-to-add selection to the new side", () => {
    const s = mountUnified([ROWS]);
    const r = rangeOf(textNode(s, 0), 6, textNode(s, 1), 5);
    expect(selectionTarget(r)).toEqual({
      file: "src/a.rs",
      side: "new",
      at: {
        selection: { start_line: 12, start_char: 6, end_line: 13, end_char: 5 },
      },
    });
  });

  it("maps a del-to-context selection to the old side", () => {
    const s = mountUnified([ROWS]);
    const r = rangeOf(textNode(s, 3), 0, textNode(s, 4), 4);
    // The old side is always commentable — the caller maps it to a
    // stored (revision, side) for the range it was drawn in (lib/comments).
    expect(selectionTarget(r)).toEqual({
      file: "src/a.rs",
      side: "old",
      at: {
        selection: { start_line: 11, start_char: 0, end_line: 12, end_char: 4 },
      },
    });
  });

  it("rejects an add-to-del selection (no side owns the text)", () => {
    const s = mountUnified([ROWS]);
    const r = rangeOf(textNode(s, 2), 0, textNode(s, 3), 4);
    expect(selectionTarget(r)).toEqual({ miss: "mixed-sides" });
  });

  it("ends a selection reaching a line's first char on the previous line", () => {
    const s = mountUnified([ROWS]);
    // Triple-click shape (browsers land the selection end there).
    const r = rangeOf(textNode(s, 1), 0, textNode(s, 2), 0);
    expect(selectionTarget(r)).toEqual({
      file: "src/a.rs",
      side: "new",
      at: {
        selection: {
          start_line: 13,
          start_char: 0,
          end_line: 13,
          end_char: "added line one".length,
        },
      },
    });
  });

  it("degrades a collapsed selection to a plain line comment", () => {
    const s = mountUnified([ROWS]);
    const t = textNode(s, 2);
    expect(selectionTarget(rangeOf(t, 3, t, 3))).toEqual({
      file: "src/a.rs",
      side: "new",
      at: { whole: 14 },
    });
  });

  it("clamps a boundary in the sign span to the line start", () => {
    const s = mountUnified([ROWS]);
    const signCell = s.querySelectorAll(".sign")[1];
    if (!signCell) throw new Error("no second .sign cell");
    const sign = signCell.firstChild as Text;
    const r = rangeOf(sign, 0, textNode(s, 1), 5);
    expect(selectionTarget(r)).toEqual({
      file: "src/a.rs",
      side: "new",
      at: {
        selection: { start_line: 13, start_char: 0, end_line: 13, end_char: 5 },
      },
    });
  });

  it("rejects a selection spanning a hunk gap", () => {
    const s = mountUnified([
      [{ new: 13, text: "first hunk" }],
      [{ new: 20, text: "second hunk" }],
    ]);
    const r = rangeOf(textNode(s, 0), 0, textNode(s, 1), 4);
    expect(selectionTarget(r)).toEqual({ miss: "hunk-gap" });
  });

  it("is silent for a selection outside any diff", () => {
    const div = document.createElement("div");
    div.textContent = "not a diff";
    document.body.appendChild(div);
    const t = div.firstChild as Text;
    expect(selectionTarget(rangeOf(t, 0, t, 5))).toBeNull();
  });

  it("walks past empty lines when the selection ends at a line start", () => {
    // Empty lines render the zero-width-space placeholder, which is row
    // chrome, not text: a selection sweeping one and ending at the next
    // line's first char must end on the last *content* line.
    const s = mountUnified([
      [
        { new: 13, text: "added line one" },
        { new: 14, text: "​" },
        { new: 15, text: "tail" },
      ],
    ]);
    const r = rangeOf(textNode(s, 0), 0, textNode(s, 2), 0);
    expect(selectionTarget(r)).toEqual({
      file: "src/a.rs",
      side: "new",
      at: {
        selection: {
          start_line: 13,
          start_char: 0,
          end_line: 13,
          end_char: "added line one".length,
        },
      },
    });
  });

  it("degrades an empty-line-only selection to a plain line comment", () => {
    const s = mountUnified([[{ new: 14, text: "​" }]]);
    const t = textNode(s, 0);
    expect(selectionTarget(rangeOf(t, 0, t, 1))).toEqual({
      file: "src/a.rs",
      side: "new",
      at: { whole: 14 },
    });
  });
});

function splitRow(
  left: { old: number; text: string } | null,
  right: { new: number; text: string } | null,
): string {
  const code = (
    sideAttr: string,
    lineAttr: string,
    cell: { text: string } | null,
  ) =>
    `<span class="code half" data-side="${sideAttr}"${lineAttr}>` +
    (cell ? `<span class="code-text">${cell.text}</span>` : "") +
    `</span>`;
  return (
    `<div class="line-row">` +
    `<span class="g" data-side="old">${left?.old ?? ""}</span>` +
    code("old", left ? ` data-old="${left.old}"` : "", left) +
    `<span class="g" data-side="new">${right?.new ?? ""}</span>` +
    code("new", right ? ` data-new="${right.new}"` : "", right) +
    `</div>`
  );
}

describe("selectionTarget, split layout", () => {
  function mountSplit(): HTMLElement {
    return mountSection(
      "src/b.rs",
      "diff-grid-split",
      splitRow(
        { old: 20, text: "left twenty" },
        { new: 30, text: "right thirty" },
      ) +
        splitRow(null, { new: 31, text: "right thirty one" }) +
        splitRow(
          { old: 21, text: "left twenty one" },
          { new: 32, text: "right thirty two" },
        ),
    );
  }

  function colText(section: HTMLElement, side: string, i: number): Text {
    const cells = section.querySelectorAll(`.code[data-side="${side}"]`);
    const cell = cells[i];
    if (!cell) throw new Error(`no ${side} cell at row ${i}`);
    const code = cell.querySelector(".code-text");
    if (!code) throw new Error(`no .code-text in ${side} cell ${i}`);
    return code.firstChild as Text;
  }

  it("maps a right-column drag across an intervening left column", () => {
    const s = mountSplit();
    // The DOM range sweeps the left cells in between; they are not part
    // of the new side's text and must be dropped, not treated as mixed.
    const r = rangeOf(colText(s, "new", 0), 2, colText(s, "new", 2), 5);
    expect(selectionTarget(r)).toEqual({
      file: "src/b.rs",
      side: "new",
      at: {
        selection: { start_line: 30, start_char: 2, end_line: 32, end_char: 5 },
      },
    });
  });

  it("maps a left-column drag across a void row (old side stays contiguous)", () => {
    const s = mountSplit();
    const r = rangeOf(colText(s, "old", 0), 1, colText(s, "old", 2), 4);
    expect(selectionTarget(r)).toEqual({
      file: "src/b.rs",
      side: "old",
      at: {
        selection: { start_line: 20, start_char: 1, end_line: 21, end_char: 4 },
      },
    });
  });

  it("rejects a drag between the two columns", () => {
    const s = mountSplit();
    const r = rangeOf(colText(s, "old", 0), 1, colText(s, "new", 2), 4);
    expect(selectionTarget(r)).toEqual({ miss: "mixed-sides" });
  });

  describe("selectionAnchorSide", () => {
    it("reads the side of the cell the anchor sits in", () => {
      const s = mountSplit();
      expect(selectionAnchorSide(colText(s, "old", 0))).toBe("old");
      expect(selectionAnchorSide(colText(s, "new", 1))).toBe("new");
    });

    it("is null off the split cells (unified view, blank anchor)", () => {
      const s = mountUnified([ROWS]);
      // Unified cells carry data-old/data-new but no data-side column tag.
      expect(selectionAnchorSide(textNode(s, 0))).toBeNull();
      expect(selectionAnchorSide(null)).toBeNull();
      const loose = document.createElement("div");
      loose.textContent = "elsewhere";
      document.body.appendChild(loose);
      expect(selectionAnchorSide(loose.firstChild)).toBeNull();
    });
  });
});

describe("linked lines", () => {
  const rows: RowSpec[] = [
    { old: 4, new: 4, text: "fn a() {" },
    { old: 5, text: "    gone();" },
    { new: 5, text: "    added();" },
    { old: 6, new: 6, text: "}" },
  ];

  it("reads a cell's line, on the new side where the cell shows both", () => {
    const section = mountUnified([rows]);
    const cells = section.querySelectorAll(".code-text");
    expect([...cells].map((c) => cellLine(c))).toEqual([
      { file: "src/a.rs", side: "new", line: 4 },
      { file: "src/a.rs", side: "old", line: 5 },
      { file: "src/a.rs", side: "new", line: 5 },
      { file: "src/a.rs", side: "new", line: 6 },
    ]);
  });

  it("spans whole lines on the side the link names", () => {
    mountUnified([rows]);
    const range = linesRange("src/a.rs", "old", 4, 6);
    if (range === null) throw new Error("no range");
    expect(selectionTarget(range, "old")).toEqual({
      file: "src/a.rs",
      side: "old",
      at: {
        selection: { start_line: 4, start_char: 0, end_line: 6, end_char: 1 },
      },
    });
  });

  it("finds no range when an end line does not render", () => {
    mountUnified([rows]);
    expect(linesRange("src/a.rs", "new", 4, 9)).toBeNull();
  });
});

describe("a rendered Markdown block", () => {
  const anchor = {
    selection: { start_line: 3, start_char: 0, end_line: 4, end_char: 8 },
  };
  const mountDoc = () => {
    const section = document.createElement("section");
    section.setAttribute("data-diff-path", "docs/a.md");
    section.innerHTML =
      `<div class="doc-block" data-side="new" data-part data-start="1"` +
      ` data-end="1" data-end-char="7"><h1>Title</h1></div>` +
      `<div class="doc-block" data-side="new" data-part data-start="3"` +
      ` data-end="4" data-end-char="8"><p>Some <em>text</em></p></div>` +
      `<div class="doc-block" data-side="new" data-part data-start="6"` +
      ` data-end="7" data-end-char="11"><table>` +
      `<tr data-part data-start="6" data-end="6" data-end-char="11">` +
      `<td data-part data-start="6" data-end="6" data-chars="2-3">a</td>` +
      `<td data-part data-start="6" data-end="6" data-chars="6-9">bcd</td>` +
      `</tr></table></div>` +
      `<div class="doc-block" data-side="new" data-part data-start="9"` +
      ` data-end="12" data-end-char="3"><pre><code>` +
      `<span data-part data-start="10" data-end="10">let a;</span>\n` +
      `<span data-part data-start="11" data-end="11">let b;</span>\n` +
      `</code></pre></div>`;
    document.body.appendChild(section);
    return section;
  };
  const target = { file: "docs/a.md", side: "new", at: anchor };

  it("targets the whole block for a selection inside it", () => {
    const em = mountDoc().querySelector("em")?.firstChild;
    if (!em) throw new Error("no text");
    expect(selectionTarget(rangeOf(em, 0, em, 2))).toEqual(target);
  });

  it("targets the whole lines of every block that a selection touches", () => {
    const section = mountDoc();
    const h1 = section.querySelector("h1")?.firstChild;
    const em = section.querySelector("em")?.firstChild;
    if (!h1 || !em) throw new Error("no text");
    expect(selectionTarget(rangeOf(h1, 2, em, 2))).toEqual({
      ...target,
      at: { selection: { ...anchor.selection, start_line: 1 } },
    });
  });

  it("targets the table cell that contains a selection", () => {
    const cell = mountDoc().querySelectorAll("td")[1]?.firstChild;
    if (!cell) throw new Error("no cell");
    expect(selectionTarget(rangeOf(cell, 0, cell, 2))).toEqual({
      ...target,
      at: {
        selection: { start_line: 6, start_char: 6, end_line: 6, end_char: 9 },
      },
    });
  });

  it("targets the row that contains a selection over two cells", () => {
    const [a, bcd] = mountDoc().querySelectorAll("td");
    if (!a?.firstChild || !bcd?.firstChild) throw new Error("no cells");
    expect(
      selectionTarget(rangeOf(a.firstChild, 0, bcd.firstChild, 1)),
    ).toEqual({ ...target, at: { whole: 6 } });
  });

  it("targets the code line that contains a selection", () => {
    const line = mountDoc().querySelectorAll("code span")[1]?.firstChild;
    if (!line) throw new Error("no code line");
    expect(selectionTarget(rangeOf(line, 0, line, 3))).toEqual({
      ...target,
      at: { whole: 11 },
    });
  });

  it("targets the blocks whose lines a link spans", () => {
    mountDoc();
    const linked = (start: number, end: number) => {
      const range = linesRange("docs/a.md", "new", start, end);
      return range && selectionTarget(range, "new");
    };
    expect(linked(3, 4)).toEqual(target);
    expect(linked(1, 4)).toEqual({
      ...target,
      at: { selection: { ...anchor.selection, start_line: 1 } },
    });
    expect(linked(3, 3)).toBeNull();
    expect(linked(11, 11)).toEqual({ ...target, at: { whole: 11 } });
  });
});
