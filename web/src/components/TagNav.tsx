import {
  type CSSProperties,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Link } from "react-router-dom";
import type { ChangeGraph, Tags } from "../api/types";
import type { NodeActivity } from "../lib/comments";
import { LAYOUT_DENSE, type LaidNode, layoutGraph } from "../lib/graphLayout";
import { StatusDot } from "./badges";
import { GraphRail } from "./GraphTable";
import Select from "./Select";

/** Rows the scroll window shows before the reviewer asks for the whole
 * graph. */
const WINDOW = 7;

/**
 * The review header's graph of the changes that share a tag with the
 * current one: the tag graph (api/fold tagGraph) drawn as the dashboard's
 * rail beside one-line rows (status dot, subject, revision, unresolved
 * count), the current change highlighted and the others linking through.
 * The selector picks which of the current change's tag keys the graph
 * follows. At first the graph scrolls inside a seven-row window centered
 * on the current change, and "show all" grows the window to the whole
 * graph.
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
  const scroller = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    // The browser clamps scrollTop, so a change near either end of the
    // graph lands the window on that end.
    if (!expanded && scroller.current !== null)
      scroller.current.scrollTop =
        (position - Math.floor(WINDOW / 2)) * layout.rowH;
  }, [expanded, position, layout.rowH]);
  const style = {
    height: layout.height,
    "--rail-w": `${layout.railWidth}px`,
    "--row-h": `${layout.rowH}px`,
  } as CSSProperties;

  return (
    <section className="tag-nav">
      <div className="tag-nav-title">
        <Select
          label="Tag"
          title="Graph the changes that share this tag's value"
          placeholder="no tags"
          value={selectedKey ?? ""}
          onChange={onSelectKey}
          options={Object.entries(tags).map(([key, value]) => ({
            value: key,
            label: key,
            detail: value,
          }))}
        />
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
      <div
        ref={scroller}
        style={
          expanded
            ? undefined
            : {
                maxHeight: WINDOW * layout.rowH,
                overflowY: "auto",
                // A wheel at either end of the window stops there, not in
                // the page.
                overscrollBehavior: "contain",
              }
        }
      >
        <div className="graph-body" style={style}>
          <GraphRail layout={layout} />
          {layout.nodes.map((ln) => (
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
  const unresolved = act?.unresolved ?? 0;
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
