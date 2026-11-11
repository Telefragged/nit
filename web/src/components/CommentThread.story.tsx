import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import {
  draft,
  reviewCtx,
  threadComment,
  uiThread,
} from "../api/fixtures/builders";
import { ReviewContext } from "../pages/reviewContext";
import CommentThread from "./CommentThread";

const queryClient = new QueryClient();
const ctx = reviewCtx({ selected: 1, latestRevision: 1 });

/** A thread as the unified diff shows it under a line. */
function Frame({ children }: { children: ReactNode }) {
  return (
    <MemoryRouter>
      <QueryClientProvider client={queryClient}>
        <ReviewContext.Provider value={ctx}>
          <div style={{ width: 840 }}>{children}</div>
        </ReviewContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>
  );
}

const base = uiThread({ id: 3 });

const ATOMIC = threadComment(
  21 * 60,
  "Generate-then-mark is not atomic. A crash between these two statements hands out a token that the store never recorded.",
);

/** A thread that the reviewer wrote and did not submit. */
export function Draft() {
  return (
    <Frame>
      <CommentThread
        thread={uiThread({
          id: null,
          drafts: [
            draft({
              body: "The old signature returned `Token` directly. Make sure that no caller unwraps the `Result` silently.",
            }),
          ],
        })}
      />
    </Frame>
  );
}

/** An open thread with a reply of the author, ported from revision 0. */
export function Open() {
  return (
    <Frame>
      <CommentThread
        thread={uiThread({
          id: 3,
          revision: 0,
          ported: { revision: 1, anchor: base.anchor },
          comments: [
            ATOMIC,
            threadComment(
              60,
              "Both statements run in one transaction, and `mark_rotated` takes the connection.",
              "author",
            ),
          ],
        })}
      />
    </Frame>
  );
}

/** A resolved thread starts as one row. */
export function Resolved() {
  return (
    <Frame>
      <CommentThread
        thread={uiThread({
          id: 4,
          resolved: true,
          comments: [
            threadComment(
              21 * 60,
              "This unwrap is a production panic on any unknown token. Return a typed error and map it to 401 at the edge.",
            ),
            threadComment(
              60,
              "`lookup()` returns a typed `RotateError`.",
              "author",
            ),
          ],
        })}
      />
    </Frame>
  );
}

/** An open thread that the reviewer resolved and did not submit. */
export function ResolveDrafted() {
  return (
    <Frame>
      <CommentThread
        thread={uiThread({
          id: 5,
          comments: [ATOMIC],
          drafts: [draft({ id: 2, thread_id: 5, resolved: true })],
        })}
      />
    </Frame>
  );
}
