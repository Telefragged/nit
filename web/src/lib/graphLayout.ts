// The change-graph layout pass — pure, kept separate from the render so the
// geometry stays unit-testable. Input: a RepoGraph (nodes already in
// topological row order, docs/api.md "Graph"). Output: positioned nodes and
// edge paths, ready for the SVG renderer.
//
// Lanes are assigned gleisbau-style (git-graph 0.7's interval-graph coloring):
// the canonical spine is pinned to lane 0 (the center column), every other
// branch is a row span packed into the first lane (1, 2, …) whose occupants
// don't overlap it. The row coordinate is the array index — children sit above
// their parents, so open changes ascend from the HEAD anchor and merged
// history descends below it. An open change whose base sits behind HEAD (main
// advanced without a rebase) attaches to that older base with a "behind" edge.

import type { GraphNode, RepoGraph } from "../api/types";

/** Visual constants for the change-graph layout (the approved "trunk &
 * branches" design: dense rows, hollow ringed nodes, elbow connectors). */
export const LAYOUT_B = {
  /** Row height; the SVG node centers align to each table row's center. */
  rowH: 46,
  /** Center of lane 0 (the canonical spine) from the rail's left edge. */
  railPadL: 42,
  /** Horizontal gap between lane centers. */
  laneGap: 42,
  /** Padding right of the last lane. */
  railPadR: 26,
  /** Node radius. */
  nodeR: 5,
  /** Extra radius for a merge node. */
  mergeBump: 1.5,
  /** Elbow quarter-circle radius for a cross-lane connector. */
  elbow: 9,
  /** Per-row opacity falloff for merged history. */
  fadeStep: 0.13,
  /** Opacity floor for faded merged history. */
  fadeFloor: 0.3,
};

export interface LaidNode {
  node: GraphNode;
  row: number;
  lane: number;
  cx: number;
  cy: number;
  /** Radius including the merge bump. */
  r: number;
  isHead: boolean;
  isMerge: boolean;
  /** Rows below the HEAD anchor (0 off the history region) — drives the fade. */
  depth: number;
  /** Number of children in the graph (a tip — the breadcrumb anchor — has 0). */
  childCount: number;
  opacity: number;
}

type EdgeKind = "open" | "history" | "behind";

export interface LaidEdge {
  key: string;
  /** SVG path `d` from child (top) to parent (bottom). */
  d: string;
  kind: EdgeKind;
  /** The branch lane the edge belongs to (the child's lane) — drives its
   * color, so a whole branch carries one color (gleisbau-style). Ignored for
   * `history` edges, which are always grey. */
  lane: number;
  opacity: number;
}

export interface GraphLayout {
  nodes: LaidNode[];
  edges: LaidEdge[];
  /** Full rail width (the GRAPH column), so the SVG can size itself. */
  railWidth: number;
  /** Full rail height = rows × rowH (includes the collapsed-history row). */
  height: number;
  rowH: number;
  /** Row of the HEAD anchor, or -1 when the graph has no head. */
  anchorRow: number;
  /** The "earlier history hidden" marker when the window is truncated: the
   * spine continues into it and deep-behind forks dangle to it. Its opacity is
   * the next grey-gradient step past the deepest spine node. Null otherwise. */
  collapsed: { cx: number; cy: number; opacity: number } | null;
}

interface Branch {
  rows: number[];
  top: number;
  bot: number;
}

/** Lay a repo graph out for rendering. Pure: never mutates `graph`. */
export function layoutGraph(graph: RepoGraph): GraphLayout {
  const nodes = graph.nodes;
  const n = nodes.length;

  const rowOf = new Map<string, number>();
  nodes.forEach((nd, i) => rowOf.set(nd.commit_sha, i));

  // In-set parents (drawable edges) per node, and the inverse children.
  const parentRows: number[][] = nodes.map((nd) =>
    nd.parents
      .map((p) => rowOf.get(p))
      .filter((r): r is number => r !== undefined),
  );
  const childRows: number[][] = nodes.map(() => []);
  parentRows.forEach((ps, i) => {
    for (const p of ps) childRows[p]?.push(i);
  });
  for (const cs of childRows) cs.sort((a, b) => a - b);

  const parentsAt = (i: number): number[] => parentRows[i] ?? [];
  const childrenAt = (i: number): number[] => childRows[i] ?? [];
  const firstParent = (i: number): number => parentsAt(i)[0] ?? -1;

  const lane = new Array<number>(n).fill(0);
  const anchorRow = nodes.findIndex((nd) => nd.section === "head");
  // The collapsed-history marker row (one below the last node), or -1.
  const markerRow = graph.history_truncated && n > 0 ? n : -1;

  // 1. The canonical spine (lane 0): from the anchor, down the first-parent
  //    chain and up the primary (smallest-row) child chain. With no anchor,
  //    the first-parent chain from the top row.
  const spine = new Set<number>();
  let cur = anchorRow >= 0 ? anchorRow : 0;
  while (cur >= 0 && cur < n && !spine.has(cur)) {
    spine.add(cur);
    cur = firstParent(cur);
  }
  if (anchorRow >= 0) {
    cur = anchorRow;
    for (;;) {
      const kid = childrenAt(cur).find(
        (c) => firstParent(c) === cur && !spine.has(c),
      );
      if (kid === undefined) break;
      spine.add(kid);
      cur = kid;
    }
  }

  // 2. Decompose the rest into branches: each non-spine node walks down its
  //    first-parent chain (claiming nodes) until it meets a claimed one.
  const branches: Branch[] = [];
  const claimed = new Set<number>(spine);
  for (let i = 0; i < n; i++) {
    if (claimed.has(i)) continue;
    const rows: number[] = [];
    let node = i;
    while (node >= 0 && !claimed.has(node)) {
      claimed.add(node);
      rows.push(node);
      node = firstParent(node);
    }
    let top = Math.min(...rows);
    let bot = Math.max(...rows);
    if (node >= 0) {
      // The connecting edge down to the fork reserves the lane to its row.
      bot = Math.max(bot, node);
    } else {
      // A deep-behind open fork dangles to the collapsed marker — reserve the
      // lane all the way down so the merged history doesn't reuse it.
      const deepest = rows[rows.length - 1];
      const deep = deepest === undefined ? undefined : nodes[deepest];
      if (
        markerRow >= 0 &&
        deep?.section === "open" &&
        deep.parents.length > 0
      ) {
        bot = markerRow;
      }
    }
    // …and a cross-lane child entering the top (a merge) reserves it upward.
    const head = rows[0];
    if (head !== undefined) {
      for (const c of childrenAt(head))
        if (!rows.includes(c)) top = Math.min(top, c);
    }
    branches.push({ rows, top, bot });
  }

  // 3. Interval-graph coloring into lanes ≥ 1 (longest span first, then by top).
  const ordered = [...branches].sort((a, b) => {
    const la = a.bot - a.top;
    const lb = b.bot - b.top;
    return lb !== la ? lb - la : a.top - b.top;
  });
  const laneSpans: [number, number][][] = [];
  for (const br of ordered) {
    let placed = laneSpans.findIndex(
      (spans) => !spans.some(([s, e]) => br.top <= e && br.bot >= s),
    );
    if (placed < 0) {
      placed = laneSpans.length;
      laneSpans.push([]);
    }
    laneSpans[placed]?.push([br.top, br.bot]);
    for (const r of br.rows) lane[r] = placed + 1;
  }

  // 4. Coordinates, nodes, edges.
  const laneAt = (i: number): number => lane[i] ?? 0;
  const cx = (l: number): number => LAYOUT_B.railPadL + l * LAYOUT_B.laneGap;
  const cy = (r: number): number => r * LAYOUT_B.rowH + LAYOUT_B.rowH / 2;
  const fade = (depth: number): number =>
    Math.max(LAYOUT_B.fadeFloor, 1 - depth * LAYOUT_B.fadeStep);
  const maxLane = lane.reduce((m, l) => Math.max(m, l), 0);

  // An edge from child (top) to parent (bottom): straight in-lane, else an elbow.
  const edgePath = (x0: number, y0: number, x1: number, y1: number): string => {
    if (x0 === x1) return `M ${x0} ${y0} L ${x1} ${y1}`;
    const b = LAYOUT_B.elbow;
    const sign = x1 > x0 ? 1 : -1;
    return `M ${x0} ${y0} L ${x0} ${y1 - b} Q ${x0} ${y1} ${x0 + sign * b} ${y1} L ${x1} ${y1}`;
  };

  const laidNodes: LaidNode[] = nodes.map((nd, i) => {
    const isHead = nd.section === "head";
    const isMerge = nd.parents.length > 1;
    const depth =
      nd.section === "history" && anchorRow >= 0 ? i - anchorRow : 0;
    return {
      node: nd,
      row: i,
      lane: laneAt(i),
      cx: cx(laneAt(i)),
      cy: cy(i),
      r: LAYOUT_B.nodeR + (isMerge ? LAYOUT_B.mergeBump : 0),
      isHead,
      isMerge,
      depth,
      childCount: childrenAt(i).length,
      opacity: nd.section === "history" ? fade(depth) : 1,
    };
  });

  // The collapsed-history marker: one row below the last node when the window
  // is truncated (more merged commits exist below). It continues the merged
  // grey gradient one step further (the fade of the next depth); the spine
  // descends into it and a deep-behind fork (base older than the window)
  // dangles to it.
  const spineBottom =
    markerRow >= 0
      ? laidNodes
          .filter((l) => l.lane === 0)
          .reduce<LaidNode | null>((a, b) => (a && a.row > b.row ? a : b), null)
      : null;
  const collapsed =
    markerRow >= 0
      ? {
          cx: cx(0),
          cy: cy(markerRow),
          opacity: fade((spineBottom?.depth ?? 0) + 1),
        }
      : null;
  const totalRows = markerRow >= 0 ? n + 1 : n;

  const edges: LaidEdge[] = [];
  laidNodes.forEach((ln, i) => {
    const inSet = parentsAt(i);
    for (const pr of inSet) {
      const p = laidNodes[pr];
      if (p === undefined) continue;
      let kind: EdgeKind;
      let opacity = 1;
      if (ln.node.section === "open" && p.node.section === "history") {
        kind = "behind"; // an open change forks off a commit behind HEAD
      } else if (ln.node.section === "open") {
        kind = "open"; // an open-chain edge (open → open, or up into HEAD)
      } else {
        kind = "history"; // the merged spine below HEAD
        opacity = fade(Math.max(ln.depth, p.depth));
      }
      edges.push({
        key: `${ln.node.commit_sha}>${p.node.commit_sha}`,
        d: edgePath(ln.cx, ln.cy, p.cx, p.cy),
        kind,
        lane: ln.lane,
        opacity,
      });
    }
    // A deep-behind fork: the base is older than the window, so there is no
    // in-set parent — dangle a behind edge down into the collapsed marker.
    if (
      ln.node.section === "open" &&
      inSet.length === 0 &&
      ln.node.parents.length > 0 &&
      collapsed
    ) {
      edges.push({
        key: `${ln.node.commit_sha}>collapsed`,
        d: edgePath(ln.cx, ln.cy, collapsed.cx, collapsed.cy),
        kind: "behind",
        lane: ln.lane,
        opacity: 1,
      });
    }
  });

  // The spine descends into the collapsed marker from the deepest lane-0 node.
  if (collapsed && spineBottom) {
    edges.push({
      key: "spine>collapsed",
      d: edgePath(spineBottom.cx, spineBottom.cy, collapsed.cx, collapsed.cy),
      kind: "history",
      lane: 0,
      opacity: collapsed.opacity,
    });
  }

  return {
    nodes: laidNodes,
    edges,
    railWidth:
      LAYOUT_B.railPadL + maxLane * LAYOUT_B.laneGap + LAYOUT_B.railPadR,
    height: totalRows * LAYOUT_B.rowH,
    rowH: LAYOUT_B.rowH,
    anchorRow,
    collapsed,
  };
}
