import { MemoryRouter } from "react-router-dom";
import { ago, draft, graphNode } from "../api/fixtures/builders";
import type { ChangeGraph } from "../api/types";
import type { NodeActivity } from "../lib/comments";
import GraphTable from "./GraphTable";

/** A stack of three changes on HEAD, one change that forks from an older
 * commit, and the merged history below them. */
const GRAPH: ChangeGraph = {
  history_truncated: true,
  nodes: [
    graphNode({
      sha: "c3",
      parent: "c2",
      change: 12,
      subject: "auth: document rotation and ship flow diagram",
    }),
    graphNode({
      sha: "c2",
      parent: "c1",
      change: 11,
      subject: "auth: rotate refresh tokens on use",
      status: "changes_requested",
    }),
    graphNode({
      sha: "c1",
      parent: "a0",
      change: 10,
      subject: "auth: add TokenStore schema and config plumbing",
      status: "approved",
    }),
    graphNode({
      sha: "d1",
      parent: "a1",
      change: 9,
      subject: "wal: checkpoint on idle, not on every commit",
      status: "commented",
    }),
    graphNode({
      sha: "a0",
      parent: "a1",
      section: "head",
      subject: "feat: make the push base optional",
    }),
    graphNode({
      sha: "a1",
      parent: "a2",
      subject: "feat: decouple display fields",
    }),
    graphNode({
      sha: "a2",
      parent: "a3",
      subject: "fix: reject an already-merged push",
    }),
  ],
};

const thread = (id: number, resolved: boolean) => ({
  id,
  change_number: 11,
  revision: 1,
  anchor: "change" as const,
  resolved,
  comments: [],
  created_at: ago(60),
  updated_at: ago(60),
});

const ACTIVITY = new Map<number, NodeActivity>([
  [
    11,
    {
      threads: [thread(1, false), thread(2, false), thread(3, true)],
      drafts: [draft({ id: 1 }), draft({ id: 2 })],
      unresolved: 2,
      decision: null,
    },
  ],
  [12, { threads: [], drafts: [], unresolved: 0, decision: "approve" }],
  [
    9,
    { threads: [thread(4, true)], drafts: [], unresolved: 0, decision: null },
  ],
]);

/** The change graph of a repository, with the activity of three changes. */
export function Default() {
  return (
    <MemoryRouter>
      <div style={{ width: 1120 }}>
        <GraphTable graph={GRAPH} activity={ACTIVITY} />
      </div>
    </MemoryRouter>
  );
}
