import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import {
  add,
  attachedFile,
  ctx,
  del,
  draft,
  reviewCtx,
  threadComment,
  uiThread,
} from "../../api/fixtures/builders";
import type { Anchor, DiffFile } from "../../api/types";
import type { LineOrRange, UiThread } from "../../lib/comments";
import { ReviewContext, type ReviewCtx } from "../../pages/reviewContext";
import DiffFileView from "./DiffFileView";

const queryClient = new QueryClient();
const review = reviewCtx();
const PATH = "src/auth/rotate.rs";

const FILE: DiffFile = {
  path: PATH,
  status: "modified",
  binary: false,
  additions: 13,
  deletions: 2,
  old_total: 23,
  new_total: 34,
  hunks: [
    {
      old_start: 18,
      old_lines: 6,
      new_start: 18,
      new_lines: 17,
      header: "impl TokenRotator",
      lines: [
        ctx(18, 18, "impl TokenRotator {"),
        ctx(19, 19, "    /// Exchange `presented` for a fresh refresh token."),
        del(20, "    pub fn rotate(&self, presented: &str) -> Token {"),
        add(
          20,
          "    pub fn rotate(&self, presented: &str) -> Result<Token, RotateError> {",
        ),
        del(
          21,
          "        self.store.swap(presented, Token::generate(&mut self.rng.lock()))",
        ),
        add(21, "        let entry = self"),
        add(22, "            .store"),
        add(23, "            .lookup(presented)"),
        add(24, "            .ok_or(RotateError::UnknownToken)?;"),
        add(25, "        if entry.rotated_at.is_some() {"),
        add(
          26,
          "            // Reuse detected: revoke the whole family (RFC 6819 §5.2.2.3).",
        ),
        add(27, "            self.store.revoke_family(entry.family_id);"),
        add(28, "            return Err(RotateError::ReuseDetected);"),
        add(29, "        }"),
        add(30, "        let fresh = Token::generate(&mut self.rng.lock());"),
        add(31, "        self.store.mark_rotated(entry.id, &fresh);"),
        add(32, "        Ok(fresh)"),
        ctx(22, 33, "    }"),
        ctx(23, 34, "}"),
      ],
    },
  ],
};

const at = (line: LineOrRange): Anchor => ({
  file: { version: "r1", path: PATH, anchor: line },
});

/** A draft on a line, a resolved thread, and an open thread on a selection
 * of two lines. */
const THREADS: UiThread[] = [
  uiThread({
    id: null,
    anchor: at({ line: 20 }),
    drafts: [
      draft({
        revision: 1,
        anchor: { file: { path: PATH, side: "new", anchor: { line: 20 } } },
        body: "Every caller has to handle the `Result`. Make sure that none unwraps it silently.",
      }),
    ],
  }),
  uiThread({
    id: 4,
    resolved: true,
    anchor: at({ line: 24 }),
    comments: [
      threadComment(
        21 * 60,
        "An unknown token must not panic. Return a typed error and map it to 401 at the edge.",
      ),
      threadComment(60, "`lookup()` returns a typed `RotateError`.", "author"),
    ],
  }),
  uiThread({
    id: 3,
    anchor: at({
      range: { start_line: 30, start_char: 8, end_line: 31, end_char: 50 },
    }),
    comments: [
      threadComment(
        21 * 60,
        "Generate-then-mark is not atomic. A crash between these two statements hands out a token that the store never recorded.",
      ),
      threadComment(
        60,
        "Both statements run in one transaction, and `mark_rotated` takes the connection.",
        "author",
      ),
    ],
  }),
];

/** A thread on the whole file. */
const FILE_THREAD = uiThread({
  id: 5,
  anchor: { file: { version: "r1", path: PATH, anchor: "file" } },
  comments: [
    threadComment(
      30,
      "Move the rotation into its own module, beside the store.",
    ),
  ],
});

const DOC_LINES = [
  "# Token rotation",
  "",
  "The store **rotates** a refresh token once,",
  "and revokes the family of a reused token.",
  "",
  "- Keep the `lookup` and the swap in one transaction.",
];
const DOC = attachedFile("plan.md", DOC_LINES.join("\n"), 1);
const charOf = (line: number, word: string) =>
  (DOC_LINES[line - 1] ?? "").indexOf(word);
const DOC_REVIEW = reviewCtx({
  editingTarget: {
    file: DOC.path,
    side: "new",
    at: {
      range: {
        start_line: 3,
        start_char: charOf(3, "rotates"),
        end_line: 4,
        end_char: charOf(4, "family") + "family".length,
      },
    },
  },
});

function Section({
  layout,
  file = FILE,
  threads = THREADS,
  value = review,
}: {
  layout: "unified" | "split";
  file?: DiffFile;
  threads?: UiThread[];
  value?: ReviewCtx;
}) {
  const [collapsed, setCollapsed] = useState(false);
  return (
    <MemoryRouter>
      <QueryClientProvider client={queryClient}>
        <ReviewContext.Provider value={value}>
          <div className="diff-column" style={{ width: 1160 }}>
            <DiffFileView
              file={file}
              layout={layout}
              threads={threads}
              domId="file-0"
              collapsed={collapsed}
              onToggle={() => {
                setCollapsed((was) => !was);
              }}
              reveal={null}
              onRevealed={() => undefined}
            />
          </div>
        </ReviewContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>
  );
}

/** One file of a diff in one column, with a thread of each state. */
export function Unified() {
  return <Section layout="unified" />;
}

/** The same file with the old and the new side in two columns. */
export function Split() {
  return <Section layout="split" />;
}

/** A thread on the whole file. */
export function FileThread() {
  return <Section layout="unified" threads={[FILE_THREAD]} />;
}

export function MarkdownSelection() {
  return (
    <Section layout="unified" file={DOC} threads={[]} value={DOC_REVIEW} />
  );
}
