import { type CSSProperties, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import type { ChangeGraph, Tags } from "../api/types";
import { type NodeActivity, revisionActivity } from "../lib/comments";
import { LAYOUT_DENSE, type LaidNode, layoutGraph } from "../lib/graphLayout";
import { StatusDot } from "./badges";
import { GraphRail } from "./GraphTable";

/** Rows shown before the reviewer asks for the whole graph. */
const WINDOW = 7;

/** The first row of the window: `WINDOW` rows around `current`, moved up
 * or down as far as needed to stay inside `total`. */
function windowStart(current: number, total: number): number {
  const half = Math.floor(WINDOW / 2);
  return Math.max(0, Math.min(current - half, total - WINDOW));
}

/**
 * The review header's graph of the changes that share a tag with the
 * current one: the tag graph (api/fold tagGraph) drawn as the dashboard's
 * rail beside one-line rows (status dot, subject, revision, unresolved
 * count), the current change highlighted and the others linking through.
 * The selector picks which of the current change's tag keys the graph
 * follows; a change with no tags disables it. Seven rows around the current
 * change show at first, and "show all" opens the whole graph. The rail is
 * laid out over the whole graph and clipped to the window, so an edge
 * that leaves the window reads as continuing past it.
 */
export default function TagNav({
  tags,
  selectedKey,
  onSelectKey,
  graph,
  activity,
  currentId,
}: {
  /** The current change's tags: the keys the selector offers. */
  tags: Tags;
  /** The key the graph follows; null only when `tags` is empty. */
  selectedKey: string | null;
  onSelectKey: (key: string) => void;
  /** The tag graph of every change carrying the selected key's value. */
  graph: ChangeGraph;
  /** Per-change activity, keyed by change number: the unresolved count. */
  activity: Map<number, NodeActivity>;
  currentId: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const layout = useMemo(() => layoutGraph(graph, LAYOUT_DENSE), [graph]);
  const total = layout.nodes.length;
  const position = layout.nodes.findIndex(
    (ln) => ln.node.change_number === currentId,
  );
  const start = expanded ? 0 : windowStart(position, total);
  const rows = expanded
    ? layout.nodes
    : layout.nodes.slice(start, start + WINDOW);
  const posLabel = `${position < 0 ? "—" : position + 1}/${total}`;
  const style = {
    height: rows.length * layout.rowH,
    "--rail-w": `${layout.railWidth}px`,
    "--row-h": `${layout.rowH}px`,
  } as CSSProperties;

  return (
    <section className="tag-nav">
      <div className="tag-nav-title">
        <select
          className="revision-select"
          aria-label="Tag"
          title="Graph the changes that share this tag's value"
          disabled={selectedKey === null}
          value={selectedKey ?? ""}
          onChange={(e) => {
            onSelectKey(e.target.value);
          }}
        >
          {selectedKey === null ? <option value="">no tags</option> : null}
          {Object.keys(tags).map((key) => (
            <option key={key} value={key}>
              {key}
            </option>
          ))}
        </select>
        <span className="tag-nav-pos mono">{posLabel}</span>
        {total > WINDOW ? (
          <button
            className="linkish tag-nav-all"
            onClick={() => {
              setExpanded((v) => !v);
            }}
          >
            {expanded ? "show less" : "show all"}
          </button>
        ) : null}
      </div>
      <div className="graph-body" style={style}>
        <GraphRail layout={layout} style={{ top: -start * layout.rowH }} />
        {rows.map((ln) => (
          <Row
            key={ln.node.commit_sha}
            ln={ln}
            act={
              ln.node.change_number === null
                ? undefined
                : activity.get(ln.node.change_number)
            }
            current={ln.node.change_number === currentId}
          />
        ))}
      </div>
    </section>
  );
}

function Row({
  ln,
  act,
  current,
}: {
  ln: LaidNode;
  act: NodeActivity | undefined;
  current: boolean;
}) {
  const { node } = ln;
  const unresolved =
    act && node.revision !== null
      ? revisionActivity(act.threads, act.drafts, node.revision).unresolved
      : 0;
  const title = `${node.subject} · ${node.status}`;
  const inner = (
    <>
      <StatusDot status={node.status} />
      <span className="subj">{node.subject}</span>
      <span className="mono dim">
        {node.revision === null ? "" : `r${node.revision}`}
      </span>
      {unresolved > 0 ? (
        <span className="unresolved-count" title="unresolved threads">
          {unresolved} open
        </span>
      ) : null}
    </>
  );
  return current ? (
    <div
      className="tag-nav-row current"
      aria-current="page"
      title={`${title} (this change)`}
    >
      {inner}
    </div>
  ) : (
    <Link
      className="tag-nav-row"
      to={`/changes/${node.change_number}`}
      title={title}
    >
      {inner}
    </Link>
  );
}
