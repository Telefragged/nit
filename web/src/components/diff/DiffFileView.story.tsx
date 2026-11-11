import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import {
  add,
  ctx,
  del,
  draft,
  reviewCtx,
  threadComment,
  uiThread,
} from "../../api/fixtures/builders";
import type { Anchor, DiffFile, LineAnchor } from "../../api/types";
import type { UiThread } from "../../lib/comments";
import { ReviewContext } from "../../pages/reviewContext";
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

const at = (side: "old" | "new", line: LineAnchor): Anchor => ({
  line: { file: PATH, side, at: line },
});

/** A draft on a line, a resolved thread, and an open thread on a selection
 * of two lines. */
const THREADS: UiThread[] = [
  uiThread({
    id: null,
    anchor: at("new", { whole: 20 }),
    drafts: [
      draft({
        anchor: at("new", { whole: 20 }),
        body: "Every caller has to handle the `Result`. Make sure that none unwraps it silently.",
      }),
    ],
  }),
  uiThread({
    id: 4,
    resolved: true,
    anchor: at("new", { whole: 24 }),
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
    anchor: at("new", {
      selection: { start_line: 30, start_char: 8, end_line: 31, end_char: 50 },
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

function Section({ layout }: { layout: "unified" | "split" }) {
  const [collapsed, setCollapsed] = useState(false);
  return (
    <MemoryRouter>
      <QueryClientProvider client={queryClient}>
        <ReviewContext.Provider value={review}>
          <div className="diff-column" style={{ width: 1160 }}>
            <DiffFileView
              file={FILE}
              layout={layout}
              threads={THREADS}
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
