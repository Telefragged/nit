import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import { graphNode } from "../api/fixtures/builders";
import type { ChangeGraph } from "../api/types";
import type { NodeActivity } from "../lib/comments";
import TagNav from "./TagNav";

const GRAPH: ChangeGraph = {
  history_truncated: false,
  nodes: [
    graphNode({
      sha: "c12",
      parent: "c11",
      change: 12,
      subject: "auth: document rotation and ship flow diagram",
      revision: 0,
    }),
    graphNode({
      sha: "c11",
      parent: "c10",
      change: 11,
      subject: "auth: rotate refresh tokens on use",
    }),
    graphNode({
      sha: "c10",
      parent: "a0",
      change: 10,
      subject: "auth: add TokenStore schema and config plumbing",
      status: "approved",
      revision: 0,
    }),
  ],
};

const ACTIVITY = new Map<number, NodeActivity>([
  [11, { drafts: [], unresolved: 3, decision: null }],
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
