import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import { shaOf } from "../api/fixtures/builders";
import type { ChangeGraph, ChangeStatus, GraphNode } from "../api/types";
import type { NodeActivity } from "../lib/comments";
import TagNav from "./TagNav";

const node = (
  change: number,
  subject: string,
  status: ChangeStatus,
  parent: string,
): GraphNode => ({
  commit_sha: shaOf(`c${change}`),
  section: "open",
  subject,
  status,
  parents: [shaOf(parent)],
  change_number: change,
  change_id: `I${change}`,
  revision: change === 11 ? 1 : 0,
  fork_sha: shaOf("a0"),
  group: null,
});

const GRAPH: ChangeGraph = {
  history_truncated: false,
  nodes: [
    node(12, "auth: document rotation and ship flow diagram", "pending", "c11"),
    node(11, "auth: rotate refresh tokens on use", "pending", "c10"),
    node(
      10,
      "auth: add TokenStore schema and config plumbing",
      "approved",
      "a0",
    ),
  ],
};

const ACTIVITY = new Map<number, NodeActivity>([
  [11, { threads: [], drafts: [], unresolved: 3, decision: null }],
]);

const TAGS = { branch: "track/auth-rotation", "session-id": "auth-rotation" };

/** A stack of three changes that share a tag. The current change, in the
 * middle, has open threads. */
export function Default() {
  const [key, setKey] = useState("session-id");
  const [expanded, setExpanded] = useState(false);
  return (
    <MemoryRouter>
      <div style={{ display: "flex", minHeight: 150 }}>
        <TagNav
          tags={TAGS}
          selectedKey={key}
          onSelectKey={setKey}
          graph={GRAPH}
          activity={ACTIVITY}
          currentId={11}
          expanded={expanded}
          onExpandedChange={setExpanded}
        />
      </div>
    </MemoryRouter>
  );
}
