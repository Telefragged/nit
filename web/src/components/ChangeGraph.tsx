import { type CSSProperties, useMemo } from "react";
import { Link } from "react-router-dom";
import type { ChangeDetail, GraphNode, RepoGraph } from "../api/types";
import { revisionActivity } from "../lib/comments";
import type { LaidEdge, LaidNode } from "../lib/graphLayout";
import { layoutGraph } from "../lib/graphLayout";
import { useRowNav } from "../lib/useRowNav";
import { StatusChip } from "./badges";

// Distinct branch colors cycle through this many lanes (gleisbau-style: a whole
// branch carries one color); merged history is always grey.
const LANE_COLORS = 6;

function edgeClass(e: LaidEdge): string {
  if (e.kind === "history") return "graph-edge edge-history";
  const lane = `lane-${((e.lane % LANE_COLORS) + LANE_COLORS) % LANE_COLORS}`;
  return e.kind === "behind"
    ? `graph-edge edge-behind ${lane}`
    : `graph-edge ${lane}`;
}

// The spine-centered change graph (docs/api.md "Graph"): one DAG over the
// canonical branch, rendered as an SVG rail (left column) beside per-row change
// cards. The layout pass (lib/graphLayout) owns all geometry; this component
// only paints the computed coordinates and the row content.

const shortSha = (sha: string): string => sha.slice(0, 12);

/** The CSS modifier for a node's ring: its branch (lane) color, matching the
 * edges; the HEAD anchor and merged history are special-cased. The review
 * status lives in the STATUS column. */
function nodeColor(ln: LaidNode): string {
  if (ln.isHead) return "head";
  if (ln.node.section === "history") return "gray";
  return `lane-${ln.lane % LANE_COLORS}`;
}

// A node's activity badges, derived from the change's own detail (fetched per
// change off the dashboard) rather than denormalized onto the graph node:
// comment/draft/unresolved counts at the node's pinned revision plus the
// reviewer's staged decision. `detail` is undefined until that fetch resolves.
function Activity({
  node,
  detail,
}: {
  node: GraphNode;
  detail: ChangeDetail | undefined;
}) {
  if (!detail || node.revision === null) return null;
  const { threads, drafts, unresolved } = revisionActivity(
    detail.threads,
    detail.drafts,
    node.revision,
  );
  const decision = detail.draft_decision?.decision ?? null;
  if (threads === 0 && drafts === 0 && unresolved === 0 && !decision) {
    return null;
  }
  return (
    <span className="counts">
      {threads > 0 && (
        <span title="published comments">
          {threads} comment{threads > 1 ? "s" : ""}
        </span>
      )}
      {drafts > 0 && (
        <span className="draft-count" title="your drafts">
          {drafts} draft{drafts > 1 ? "s" : ""}
        </span>
      )}
      {unresolved > 0 && (
        <span className="unresolved-count" title="unresolved threads">
          {unresolved} open
        </span>
      )}
      {decision && (
        <span
          className="draft-count"
          title="your staged decision (not yet submitted)"
        >
          ✎ {decision}
        </span>
      )}
    </span>
  );
}

function GraphRow({
  ln,
  detail,
}: {
  ln: LaidNode;
  detail: ChangeDetail | undefined;
}) {
  const { node } = ln;
  const isOpen = node.section === "open";
  const isHistory = node.section === "history";
  // The whole row navigates to the change; the subject stays a link so
  // cmd/middle-click still opens a tab (useRowNav ignores it).
  const to = node.change_id !== null ? `/changes/${node.change_id}` : null;
  const rowNav = useRowNav(to ?? "");
  const subject = to ? (
    <Link to={to} className="graph-subject">
      {node.subject}
    </Link>
  ) : (
    <span className="graph-subject">{node.subject}</span>
  );
  return (
    // The whole row is a mouse shortcut to the change; the subject inside is a
    // focusable link providing the keyboard path, so the row needs no key handler.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
    <div
      className={`graph-row${isHistory ? " is-history" : ""}${to ? " is-clickable" : ""}`}
      // The post-submit navigate's `#chain-<tip>` scroll target (ReviewBar
      // lands here after publishing a chain's review) — only the tip (a leaf in
      // the open region) carries it, so a change live at two revisions
      // (B-in-two-chains) never emits a duplicate id.
      id={
        isOpen && node.change_id !== null && ln.childCount === 0
          ? `chain-${node.change_id}`
          : undefined
      }
      onClick={to ? rowNav.onClick : undefined}
      style={{ opacity: ln.opacity, cursor: to ? "pointer" : undefined }}
    >
      <div className="graph-cell-rail" aria-hidden="true" />
      <div className="graph-cell-change">
        {subject}
        <div className="graph-meta">
          <span className="mono sha">{shortSha(node.commit_sha)}</span>
        </div>
      </div>
      <div className="graph-cell-status">
        {isOpen ? (
          <StatusChip status={node.status} />
        ) : ln.isHead ? (
          <span className="graph-plain-status">HEAD</span>
        ) : (
          <span className="graph-plain-status">merged</span>
        )}
      </div>
      <div className="graph-cell-rev mono">
        {isOpen && node.revision !== null ? `r${node.revision}` : ""}
      </div>
      <div className="graph-cell-activity">
        <Activity node={node} detail={detail} />
      </div>
    </div>
  );
}

export default function ChangeGraph({
  graph,
  activity,
}: {
  graph: RepoGraph;
  /** Per-change detail, keyed by change id — the source for each node's
   * activity badges. */
  activity: Map<number, ChangeDetail>;
}) {
  const layout = useMemo(() => layoutGraph(graph), [graph]);
  const cols = `${layout.railWidth}px minmax(0, 1fr) 168px 52px 184px`;
  const bodyStyle = {
    height: layout.height,
    "--rail-w": `${layout.railWidth}px`,
    "--row-h": `${layout.rowH}px`,
    "--graph-cols": cols,
  } as CSSProperties;
  const collapsed = layout.collapsed;

  return (
    <div className="change-graph">
      <div className="graph-colhead" style={{ gridTemplateColumns: cols }}>
        <span>Graph</span>
        <span>Change</span>
        <span>Status</span>
        <span>Rev</span>
        <span>Activity</span>
      </div>
      <div className="graph-body" style={bodyStyle}>
        <svg
          className="graph-rail"
          width={layout.railWidth}
          height={layout.height}
          aria-hidden="true"
        >
          {layout.edges.map((e) => (
            <path
              key={e.key}
              className={edgeClass(e)}
              d={e.d}
              opacity={e.opacity}
            />
          ))}
          {layout.nodes.map((ln) => (
            <g key={ln.node.commit_sha} opacity={ln.opacity}>
              {ln.isHead && (
                <circle
                  className="graph-node-ring"
                  cx={ln.cx}
                  cy={ln.cy}
                  r={ln.r + 4.5}
                />
              )}
              <circle
                className={`graph-node node-${nodeColor(ln)}`}
                cx={ln.cx}
                cy={ln.cy}
                r={ln.r}
              />
            </g>
          ))}
          {collapsed && (
            <path
              className="graph-chevron"
              d={`M ${collapsed.cx - 5} ${collapsed.cy - 3} L ${collapsed.cx} ${collapsed.cy + 3} L ${collapsed.cx + 5} ${collapsed.cy - 3}`}
              opacity={collapsed.opacity}
            />
          )}
        </svg>
        {layout.nodes.map((ln) => (
          <GraphRow
            key={ln.node.commit_sha}
            ln={ln}
            detail={
              ln.node.change_id !== null
                ? activity.get(ln.node.change_id)
                : undefined
            }
          />
        ))}
        {collapsed && (
          <div
            className="graph-row is-collapsed"
            style={{ opacity: collapsed.opacity }}
          >
            <div className="graph-cell-rail" aria-hidden="true" />
            <div className="graph-cell-change">
              <span className="graph-collapsed-label">
                earlier history hidden
              </span>
            </div>
            <div />
            <div />
            <div />
          </div>
        )}
      </div>
    </div>
  );
}
