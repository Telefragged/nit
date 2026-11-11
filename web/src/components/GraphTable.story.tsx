import { MemoryRouter } from "react-router-dom";
import { ago, draft, shaOf } from "../api/fixtures/builders";
import type {
  ChangeGraph,
  ChangeStatus,
  GraphNode,
  GraphSection,
} from "../api/types";
import type { NodeActivity } from "../lib/comments";
import GraphTable from "./GraphTable";

const node = (
  sha: string,
  section: GraphSection,
  subject: string,
  status: ChangeStatus,
  parent: string,
  change: number | null,
): GraphNode => ({
  commit_sha: shaOf(sha),
  section,
  subject,
  status,
  parents: [shaOf(parent)],
  change_number: change,
  change_id: change === null ? null : `I${change}`,
  revision: change === null ? null : 1,
  fork_sha: shaOf("a0"),
  group: null,
});

/** A stack of three changes on HEAD, one change that forks from an older
 * commit, and the merged history below them. */
const GRAPH: ChangeGraph = {
  history_truncated: true,
  nodes: [
    node(
      "c3",
      "open",
      "auth: document rotation and ship flow diagram",
      "pending",
      "c2",
      12,
    ),
    node(
      "c2",
      "open",
      "auth: rotate refresh tokens on use",
      "changes_requested",
      "c1",
      11,
    ),
    node(
      "c1",
      "open",
      "auth: add TokenStore schema and config plumbing",
      "approved",
      "a0",
      10,
    ),
    node(
      "d1",
      "open",
      "wal: checkpoint on idle, not on every commit",
      "commented",
      "a1",
      9,
    ),
    node(
      "a0",
      "head",
      "feat: make the push base optional",
      "merged",
      "a1",
      null,
    ),
    node(
      "a1",
      "history",
      "feat: decouple display fields",
      "merged",
      "a2",
      null,
    ),
    node(
      "a2",
      "history",
      "fix: reject an already-merged push",
      "merged",
      "a3",
      null,
    ),
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
