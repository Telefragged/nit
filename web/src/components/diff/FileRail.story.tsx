import { useState, type ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import {
  attachedFile,
  msgFile,
  reviewCtx,
  uiThread,
} from "../../api/fixtures/builders";
import type { DiffFile, FileStatus } from "../../api/types";
import type { ReviewFile } from "../../lib/attachments";
import type { UiThread } from "../../lib/comments";
import { ReviewContext } from "../../pages/reviewContext";
import FileRail from "./FileRail";

const ctx = reviewCtx({
  linkHref: (link) =>
    "thread" in link
      ? `?thread=${link.thread}`
      : `?file=${link.file}&line=${link.lines?.start ?? ""}`,
});

/** The rail in the sidebar of the review page. */
function Frame({ children }: { children: ReactNode }) {
  return (
    <MemoryRouter>
      <ReviewContext.Provider value={ctx}>
        <div className="sidebar-layout">
          <aside className="review-sidebar">{children}</aside>
        </div>
      </ReviewContext.Provider>
    </MemoryRouter>
  );
}

const file = (
  path: string,
  status: FileStatus,
  additions: number,
  deletions: number,
  over: Partial<DiffFile> = {},
): DiffFile => ({
  path,
  status,
  binary: false,
  additions,
  deletions,
  old_total: 0,
  new_total: 0,
  hunks: [],
  ...over,
});

const FILES: ReviewFile[] = [
  msgFile("web: show the file rail in the gallery\n"),
  file("crates/nit/src/server.rs", "modified", 12, 3),
  file("crates/nit/src/scan.rs", "modified", 4, 4),
  file("web/src/components/diff/FileRail.tsx", "added", 248, 0),
  file("web/src/styles/rail.css", "deleted", 0, 61),
  file("docs/dev.md", "renamed", 2, 1, { old_path: "docs/develop.md" }),
  file("web/public/logo.png", "added", 0, 0, { binary: true }),
  attachedFile("screenshots/review.html", "<!doctype html>\n", 0),
  attachedFile("plan/rail.md", "# Rail\n", 1),
];

const line = (path: string, whole: number) => ({
  line: { file: path, side: "new" as const, at: { whole } },
});

const THREADS = new Map<string, UiThread[]>([
  [
    "crates/nit/src/server.rs",
    [
      uiThread({ id: 3, anchor: line("crates/nit/src/server.rs", 41) }),
      uiThread({
        id: 7,
        revision: 0,
        anchor: {
          line: {
            file: "crates/nit/src/server.rs",
            side: "new",
            at: {
              selection: {
                start_line: 88,
                start_char: 4,
                end_line: 90,
                end_char: 12,
              },
            },
          },
        },
      }),
      uiThread({ id: null, anchor: line("crates/nit/src/server.rs", 12) }),
    ],
  ],
  [
    "crates/nit/src/scan.rs",
    [
      uiThread({
        id: 9,
        resolved: true,
        anchor: line("crates/nit/src/scan.rs", 30),
      }),
    ],
  ],
  ["docs/dev.md", [uiThread({ id: null, anchor: line("docs/dev.md", 5) })]],
]);

/** A diff with every kind of file row, a file for each thread mark, and
 * attachments.
 * The selection and the expand-all toggle are state of the story. */
export function Default() {
  const [active, setActive] = useState<number | null>(null);
  const [allExpanded, setAllExpanded] = useState(false);
  return (
    <Frame>
      <FileRail
        files={FILES}
        threadsByFile={THREADS}
        activeIndex={active}
        onSelect={setActive}
        allExpanded={allExpanded}
        onToggleAll={() => {
          setAllExpanded((all) => !all);
        }}
      />
    </Frame>
  );
}

/** The rail while the diff loads. */
export function Empty() {
  return (
    <Frame>
      <FileRail
        files={[]}
        threadsByFile={new Map()}
        activeIndex={null}
        onSelect={() => undefined}
        allExpanded={false}
        onToggleAll={() => undefined}
      />
    </Frame>
  );
}
