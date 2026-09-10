// The change-graph layout pass — pure, kept separate from the render so the
// geometry stays unit-testable. Input: a ChangeGraph (nodes already in
// topological row order). Output: positioned nodes and
// edge paths, ready for the SVG renderer.
//
// Lanes are assigned gleisbau-style (git-graph 0.7's interval-graph coloring):
// the canonical ref is pinned to lane 0 (the center column), every other
// branch is a row span packed into the first lane (1, 2, …) whose occupants
// don't overlap it. A graph without a head (the tag graph) has no canonical
// ref, so its branches pack from lane 0. The row coordinate is the array
// index: children sit above their parents, so open changes ascend from the
// HEAD anchor and merged history descends below it. An open change attaches to its base with a solid
// edge whenever that base is a visible node (HEAD or a merged commit still in
// the window); only a base older than the window — no node to anchor to —
// dangles a dashed "behind" edge into the collapsed-history marker. An open
// change whose parent is missing from the graph attaches to its fork
// instead, and a break mark cuts that edge: the commits between the two are
// not shown. The dash and the mark are independent, so a fork below the
// window with hidden commits above it gets both. A grouped graph opens a
// gap between two rows whose group differs, so each run of a group reads as
// one block.

import type { GraphNode, ChangeGraph } from "../api/types";

/** The geometry a layout is measured in. */
export interface LayoutMetrics {
  /** The SVG node centers align to each table row's center. */
  rowH: number;
  /** Center of lane 0 from the rail's left edge. */
  railPadL: number;
  laneGap: number;
  railPadR: number;
  nodeR: number;
  /** Extra radius for a merge node. */
  mergeBump: number;
  /** Elbow quarter-circle radius for a cross-lane connector. */
  elbow: number;
  /** Per-row opacity falloff for merged history. */
  fadeStep: number;
  fadeFloor: number;
  /** The gap above a row that starts a new group. */
  gapH: number;
}

/** Visual constants for the change-graph layout (the approved "trunk &
 * branches" design: dense rows, hollow ringed nodes, elbow connectors). */
export const LAYOUT_B: LayoutMetrics = {
  rowH: 46,
  railPadL: 42,
  laneGap: 42,
  railPadR: 26,
  nodeR: 5,
  mergeBump: 1.5,
  elbow: 9,
  fadeStep: 0.13,
  fadeFloor: 0.3,
  gapH: 24,
};

/** The tag graph's metrics: one-line rows, and lanes packed close. */
export const LAYOUT_DENSE: LayoutMetrics = {
  ...LAYOUT_B,
  rowH: 24,
  railPadL: 11,
  laneGap: 14,
  railPadR: 11,
  nodeR: 4,
  mergeBump: 1,
  elbow: 6,
  gapH: 0,
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
  opacity: number;
  /** The row starts a run of a group. The row above it belongs to a
   * different group, or the row is the first row. */
  gapAbove: boolean;
}

type EdgeKind = "open" | "history" | "behind";

export interface LaidEdge {
  key: string;
  /** SVG path `d` from child (top) to parent (bottom). */
  d: string;
  kind: EdgeKind;
  /** Where the break mark sits when commits are hidden between the edge's
   * ends: on the edge's vertical run, at the boundary below the child's
   * row. Null otherwise. */
  mark: { x: number; y: number } | null;
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
  /** Full rail height: every row, gap and the collapsed-history row. */
  height: number;
  rowH: number;
  /** The gap above a row that starts a run of a group. */
  gapH: number;
  /** Row of the HEAD anchor, or -1 when the graph has no head. */
  anchorRow: number;
  /** The "earlier history hidden" marker when the window is truncated: the
   * canonical ref continues into it and deep-behind forks dangle to it. Its
   * opacity is the next grey-gradient step past its deepest node. Null
   * otherwise. */
  collapsed: { cx: number; cy: number; opacity: number } | null;
}

interface Branch {
  rows: number[];
  top: number;
  bot: number;
}

/** Pure: never mutates `graph`. */
export function layoutGraph(
  graph: ChangeGraph,
  m: LayoutMetrics = LAYOUT_B,
): GraphLayout {
  const nodes = graph.nodes;
  const n = nodes.length;

  const rowOf = new Map<string, number>();
  nodes.forEach((nd, i) => rowOf.set(nd.commit_sha, i));

  // In-set (drawable) parents, and the fork row a break attaches to.
  // childRows is the inverse of both, so a break child takes part in the
  // lane walk like any other.
  const parentRows: number[][] = nodes.map((nd) =>
    nd.parents
      .map((p) => rowOf.get(p))
      .filter((r): r is number => r !== undefined),
  );
  const broken: boolean[] = nodes.map(
    (nd, i) =>
      nd.section === "open" &&
      parentRows[i]?.length === 0 &&
      nd.fork_sha !== null &&
      nd.parents.length > 0 &&
      !nd.parents.includes(nd.fork_sha),
  );
  const breakRow: number[] = nodes.map((nd, i) =>
    broken[i] ? (rowOf.get(nd.fork_sha ?? "") ?? -1) : -1,
  );
  const childRows: number[][] = nodes.map(() => []);
  parentRows.forEach((ps, i) => {
    for (const p of ps) childRows[p]?.push(i);
    const br = breakRow[i] ?? -1;
    if (br >= 0) childRows[br]?.push(i);
  });
  for (const cs of childRows) cs.sort((a, b) => a - b);

  const parentsAt = (i: number): number[] => parentRows[i] ?? [];
  const childrenAt = (i: number): number[] => childRows[i] ?? [];
  const firstParent = (i: number): number =>
    parentsAt(i)[0] ?? breakRow[i] ?? -1;

  const lane = new Array<number>(n).fill(0);
  const anchorRow = nodes.findIndex((nd) => nd.section === "head");
  // The collapsed-history marker row (one below the last node), or -1.
  const markerRow = graph.history_truncated && n > 0 ? n : -1;

  // 1. The canonical ref: first-parent chain down from the anchor; then up
  //    via the primary (smallest-row, per the childRows sort above) child.
  //    Without a head there is no canonical ref, and no lane is reserved.
  const canonical = new Set<number>();
  if (anchorRow >= 0) {
    let cur = anchorRow;
    while (cur >= 0 && !canonical.has(cur)) {
      canonical.add(cur);
      cur = firstParent(cur);
    }
    cur = anchorRow;
    for (;;) {
      const kid = childrenAt(cur).find(
        (c) => firstParent(c) === cur && !canonical.has(c),
      );
      if (kid === undefined) break;
      canonical.add(kid);
      cur = kid;
    }
  }

  // 2. Decompose the rest into branches: each node off it walks down its
  //    first-parent chain (claiming nodes) until it meets a claimed one.
  const branches: Branch[] = [];
  const claimed = new Set<number>(canonical);
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

  // 3. Interval-graph coloring. With a head, the canonical ref owns lane 0
  //    for the whole height.
  const laneSpans: [number, number][][] =
    anchorRow >= 0 ? [[[0, Number.POSITIVE_INFINITY]]] : [];
  const ordered = [...branches].sort((a, b) => {
    const la = a.bot - a.top;
    const lb = b.bot - b.top;
    return lb !== la ? lb - la : a.top - b.top;
  });
  for (const br of ordered) {
    let placed = laneSpans.findIndex(
      (spans) => !spans.some(([s, e]) => br.top <= e && br.bot >= s),
    );
    if (placed < 0) {
      placed = laneSpans.length;
      laneSpans.push([]);
    }
    laneSpans[placed]?.push([br.top, br.bot]);
    for (const r of br.rows) lane[r] = placed;
  }

  // Row tops. The first row of a run gets a gap above it, so every run
  // carries its label. Rows past the last node (the collapsed marker)
  // follow at the plain row pitch.
  const gapAbove: boolean[] = nodes.map((nd, i) =>
    i === 0 ? nd.group !== null : nodes[i - 1]?.group !== nd.group,
  );
  const rowTop: number[] = [];
  let bottom = 0;
  nodes.forEach((_, i) => {
    if (gapAbove[i]) bottom += m.gapH;
    rowTop.push(bottom);
    bottom += m.rowH;
  });
  const top = (r: number): number => rowTop[r] ?? bottom + (r - n) * m.rowH;

  const laneAt = (i: number): number => lane[i] ?? 0;
  const cx = (l: number): number => m.railPadL + l * m.laneGap;
  const cy = (r: number): number => top(r) + m.rowH / 2;
  const fade = (depth: number): number =>
    Math.max(m.fadeFloor, 1 - depth * m.fadeStep);
  const maxLane = lane.reduce((m, l) => Math.max(m, l), 0);

  const edgePath = (x0: number, y0: number, x1: number, y1: number): string => {
    if (x0 === x1) return `M ${x0} ${y0} L ${x1} ${y1}`;
    const b = m.elbow;
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
      r: m.nodeR + (isMerge ? m.mergeBump : 0),
      isHead,
      isMerge,
      depth,
      opacity: nd.section === "history" ? fade(depth) : 1,
      gapAbove: gapAbove[i] ?? false,
    };
  });

  // The collapsed-history marker: one row below the last node when the window
  // is truncated (more merged commits exist below). It continues the merged
  // grey gradient one step further (the fade of the next depth); the canonical ref
  // descends into it and a deep-behind fork (base older than the window)
  // dangles to it.
  const canonicalBottom =
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
          opacity: fade((canonicalBottom?.depth ?? 0) + 1),
        }
      : null;

  const edges: LaidEdge[] = [];
  laidNodes.forEach((ln, i) => {
    const inSet = parentsAt(i);
    for (const pr of inSet) {
      const p = laidNodes[pr];
      if (p === undefined) continue;
      let kind: EdgeKind;
      let opacity = 1;
      if (ln.node.section === "open") {
        // An open change anchored to a visible node — its real base is on
        // screen, so the edge is solid (lane-colored), whether that base is
        // HEAD, another open commit, or a merged commit still in the window.
        kind = "open";
      } else {
        kind = "history"; // the merged canonical ref below HEAD
        opacity = fade(Math.max(ln.depth, p.depth));
      }
      edges.push({
        key: `${ln.node.commit_sha}>${p.node.commit_sha}`,
        d: edgePath(ln.cx, ln.cy, p.cx, p.cy),
        kind,
        mark: null,
        lane: ln.lane,
        opacity,
      });
    }
    if (ln.node.section !== "open" || inSet.length > 0) return;
    const isBroken = broken[i] ?? false;
    const mark = { x: ln.cx, y: ln.cy + m.rowH / 2 };
    const fork = laidNodes[breakRow[i] ?? -1];
    if (fork !== undefined) {
      // Hidden commits between the change and its visible fork.
      edges.push({
        key: `${ln.node.commit_sha}>${fork.node.commit_sha}`,
        d: edgePath(ln.cx, ln.cy, fork.cx, fork.cy),
        kind: "open",
        mark,
        lane: ln.lane,
        opacity: 1,
      });
    } else if (ln.node.parents.length > 0 && collapsed) {
      // The fork is older than the window: a dangle to the marker, marked
      // when commits are hidden above the fork too.
      edges.push({
        key: `${ln.node.commit_sha}>collapsed`,
        d: edgePath(ln.cx, ln.cy, collapsed.cx, collapsed.cy),
        kind: "behind",
        mark: isBroken ? mark : null,
        lane: ln.lane,
        opacity: 1,
      });
    }
  });

  if (collapsed && canonicalBottom) {
    edges.push({
      key: "canonical>collapsed",
      d: edgePath(
        canonicalBottom.cx,
        canonicalBottom.cy,
        collapsed.cx,
        collapsed.cy,
      ),
      kind: "history",
      mark: null,
      lane: 0,
      opacity: collapsed.opacity,
    });
  }

  return {
    nodes: laidNodes,
    edges,
    railWidth: m.railPadL + maxLane * m.laneGap + m.railPadR,
    height: bottom + (markerRow >= 0 ? m.rowH : 0),
    rowH: m.rowH,
    gapH: m.gapH,
    anchorRow,
    collapsed,
  };
}
