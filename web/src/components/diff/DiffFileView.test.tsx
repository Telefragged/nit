// Rebase-drift lines (docs/api.md "Rebase-aware interdiffs") render
// contained: the .drift class lands on the changed line's gutter and code
// cell so the CSS can tint them, while the real edit beside them stays
// untagged. Rendered with a minimal ReviewContext so the assertion is about
// the line markup alone.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { DiffFile } from "../../api/types";
import { ReviewContext, type ReviewCtx } from "../../pages/reviewContext";
import DiffFileView from "./DiffFileView";

afterEach(cleanup);

const ctx: ReviewCtx = {
  changeId: 1,
  selected: 2,
  against: 1,
  editingTarget: null,
  setEditingTarget: () => false,
  setEditorDirty: () => undefined,
};

// A mixed hunk: a real edit (line 1) and a drift edit (line 2) tagged.
const mixed: DiffFile = {
  path: "src/base.rs",
  status: "modified",
  binary: false,
  additions: 1,
  deletions: 1,
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

function renderFile(layout: "unified" | "split") {
  const queryClient = new QueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ReviewContext.Provider value={ctx}>
        <DiffFileView
          file={mixed}
          layout={layout}
          threads={[]}
          domId="file-0"
          collapsed={false}
          onToggle={() => undefined}
        />
      </ReviewContext.Provider>
    </QueryClientProvider>,
  );
}

describe("rebase drift rendering", () => {
  it("tags only the drift line's code cells in unified layout", () => {
    const { container } = renderFile("unified");
    // The two drift lines (del + add), and only those, carry .drift.
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
