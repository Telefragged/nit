import { describe, expect, it } from "vitest";
import type {
  ChangeStatus,
  GraphNode,
  GraphSection,
  RepoGraph,
} from "../api/types";
import type { GraphLayout, LaidNode } from "./graphLayout";
import { LAYOUT_B, layoutGraph } from "./graphLayout";

function must<T>(v: T | undefined, msg: string): T {
  if (v === undefined) throw new Error(`missing ${msg}`);
  return v;
}

function node(
  sha: string,
  section: GraphSection,
  status: ChangeStatus,
  parents: string[],
): GraphNode {
  return {
    commit_sha: sha,
    section,
    subject: `subject ${sha}`,
    status,
    parents,
    change_id: section === "open" ? 1 : null,
    change_key: section === "open" ? `I${sha}` : null,
    revision: section === "open" ? 0 : null,
  };
}

const find = (g: GraphLayout, sha: string): LaidNode =>
  must(
    g.nodes.find((nd) => nd.node.commit_sha === sha),
    sha,
  );
const laneOf = (g: GraphLayout, sha: string): number => find(g, sha).lane;
const edge = (g: GraphLayout, from: string, to: string) =>
  must(
    g.edges.find((e) => e.key === `${from}>${to}`),
    `${from}>${to}`,
  );

// Node order below is the row order layoutGraph keys off (array index →
// row); reordering these nodes shifts every row-based assertion below.
function mockGraph(): RepoGraph {
  return {
    repo_id: 1,
    anchor: "H",
    history_truncated: false,
    nodes: [
      node("A1", "open", "pending", ["A3"]),
      node("A2", "open", "changes_requested", ["A3"]),
      node("A3", "open", "approved", ["A4"]),
      node("A4", "open", "pending", ["H"]),
      node("H", "head", "merged", ["G1"]),
      node("G1", "history", "merged", ["G2"]),
      node("G2", "history", "merged", ["G3", "G4"]),
      node("G3", "history", "merged", ["G5"]),
      node("G4", "history", "merged", ["G5"]),
      node("G5", "history", "merged", []),
    ],
  };
}

describe("layoutGraph lanes", () => {
  it("pins the spine through HEAD to lane 0", () => {
    const g = layoutGraph(mockGraph());
    for (const sha of ["A1", "A3", "A4", "H", "G1", "G2", "G3", "G5"]) {
      expect(laneOf(g, sha)).toBe(0);
    }
  });

  it("packs side branches into lanes ≥ 1, reusing a lane when spans are disjoint", () => {
    const g = layoutGraph(mockGraph());
    expect(laneOf(g, "A2")).toBe(1);
    // Disjoint row spans ([1,2] vs [6,9]) share lane 1 — compact packing.
    expect(laneOf(g, "G4")).toBe(1);
    expect(g.railWidth).toBe(
      LAYOUT_B.railPadL + 1 * LAYOUT_B.laneGap + LAYOUT_B.railPadR,
    );
  });
});

describe("layoutGraph coordinates", () => {
  it("places row i at i*rowH + rowH/2 and lane l at railPadL + l*laneGap", () => {
    const g = layoutGraph(mockGraph());
    const a1 = find(g, "A1");
    expect(a1.cy).toBe(LAYOUT_B.rowH / 2);
    expect(a1.cx).toBe(LAYOUT_B.railPadL);
    const a2 = find(g, "A2");
    expect(a2.cy).toBe(LAYOUT_B.rowH + LAYOUT_B.rowH / 2);
    expect(a2.cx).toBe(LAYOUT_B.railPadL + LAYOUT_B.laneGap);
    expect(g.height).toBe(10 * LAYOUT_B.rowH);
  });
});

describe("layoutGraph node semantics", () => {
  it("marks the head, merges, and fan-out, and fades history by depth", () => {
    const g = layoutGraph(mockGraph());
    expect(find(g, "H").isHead).toBe(true);
    expect(g.anchorRow).toBe(4);
    expect(g.collapsed).toBeNull();

    expect(find(g, "G2").isMerge).toBe(true);
    expect(find(g, "A3").childCount).toBe(2);

    // Opacity floor 0.3 — not yet reached; G5 at depth 5 gives 0.35.
    expect(find(g, "A1").opacity).toBe(1);
    expect(find(g, "H").opacity).toBe(1);
    expect(find(g, "G1").depth).toBe(1);
    expect(find(g, "G1").opacity).toBeCloseTo(1 - 0.13);
    expect(find(g, "G5").depth).toBe(5);
    expect(find(g, "G5").opacity).toBeCloseTo(0.35);
  });
});

describe("layoutGraph edges", () => {
  it("colors open edges by lane and the merged spine as history", () => {
    const g = layoutGraph(mockGraph());
    expect(edge(g, "A4", "H").kind).toBe("open"); // open chain joins HEAD
    expect(edge(g, "A4", "H").lane).toBe(0); // on the spine
    expect(edge(g, "A2", "A3").kind).toBe("open"); // cross-lane fork
    expect(edge(g, "A2", "A3").lane).toBe(1); // A2's side lane carries the color
    expect(edge(g, "H", "G1").kind).toBe("history"); // into merged history
    expect(edge(g, "G2", "G4").kind).toBe("history");
  });
});

// An open change pushed onto an older merged commit still in the window — main
// advanced without a rebase, so the change forks off a visible history node.
// Its base is on screen, so the edge is solid (only an off-window base dashes).
describe("layoutGraph behind-HEAD base (in window)", () => {
  it("attaches to its visible base with a solid edge on a side lane", () => {
    const g = layoutGraph({
      repo_id: 1,
      anchor: "H",
      history_truncated: false,
      nodes: [
        node("J", "open", "pending", ["H"]),
        node("B", "open", "pending", ["c2e8e4d"]),
        node("H", "head", "merged", ["G1"]),
        node("G1", "history", "merged", ["c2e8e4d"]),
        node("c2e8e4d", "history", "merged", ["R"]),
        node("R", "history", "merged", []),
      ],
    });
    expect(laneOf(g, "B")).toBe(1);
    expect(edge(g, "B", "c2e8e4d").kind).toBe("open");
    expect(edge(g, "B", "c2e8e4d").lane).toBe(1); // the edge keeps B's color
    expect(edge(g, "J", "H").kind).toBe("open"); // normal fork at HEAD
    expect(g.collapsed).toBeNull();
  });
});

// The base is OLDER than the displayed window: the fork has no visible node, so
// its lineage dangles into the collapsed "earlier history hidden" marker.
describe("layoutGraph behind-HEAD base (below window)", () => {
  it("dangles a behind edge into the collapsed marker", () => {
    const g = layoutGraph({
      repo_id: 1,
      anchor: "H",
      history_truncated: true,
      nodes: [
        node("X", "open", "pending", ["DEEP"]), // DEEP is below the window
        node("H", "head", "merged", ["G1"]),
        node("G1", "history", "merged", ["G2"]),
        node("G2", "history", "merged", ["DEEP"]),
      ],
    });
    expect(g.collapsed).not.toBeNull();
    // The marker sits one row below the last node, on the spine (lane 0).
    expect(g.collapsed?.cy).toBe(4 * LAYOUT_B.rowH + LAYOUT_B.rowH / 2);
    expect(g.collapsed?.cx).toBe(LAYOUT_B.railPadL);
    expect(g.height).toBe(5 * LAYOUT_B.rowH); // 4 nodes + the marker row
    // It continues the grey gradient one step past the deepest spine node
    // (G2 at depth 2 → the marker fades at depth 3).
    expect(g.collapsed?.opacity).toBeCloseTo(1 - 3 * LAYOUT_B.fadeStep);
    // The deep fork dangles to the marker; the spine descends into it too, at
    // the same next-gradient opacity as the marker.
    expect(edge(g, "X", "collapsed").kind).toBe("behind");
    expect(edge(g, "spine", "collapsed").kind).toBe("history");
    expect(edge(g, "spine", "collapsed").opacity).toBeCloseTo(
      g.collapsed?.opacity ?? 0,
    );
  });
});

describe("layoutGraph purity", () => {
  it("does not mutate its input", () => {
    const g = mockGraph();
    const before = JSON.stringify(g);
    layoutGraph(g);
    expect(JSON.stringify(g)).toBe(before);
  });
});
