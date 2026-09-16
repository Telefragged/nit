import {
  type CSSProperties,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
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
 * follows. Seven rows around the current change show at first, and "show
 * all" opens the whole graph. The rail is laid out over the whole graph
 * and clipped to the window, so an edge that leaves the window reads as
 * continuing past it.
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
  const style = {
    height: rows.length * layout.rowH,
    "--rail-w": `${layout.railWidth}px`,
    "--row-h": `${layout.rowH}px`,
  } as CSSProperties;

  return (
    <section className="tag-nav">
      <div className="tag-nav-title">
        <TagSelect
          tags={tags}
          selectedKey={selectedKey}
          onSelectKey={onSelectKey}
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

/**
 * The selector over the current change's tag keys: each row carries its
 * key and, right-aligned and dim, the value that key holds, so the
 * reviewer sees which value the graph follows without opening the list.
 * A change with no tags disables it.
 *
 * A native `<select>` cannot lay an option out in two columns, so this is
 * a button over a list of buttons. The list drops below the head, which
 * stays put, so a second press on the head closes it again.
 */
function TagSelect({
  tags,
  selectedKey,
  onSelectKey,
}: {
  tags: Tags;
  selectedKey: string | null;
  onSelectKey: (key: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const group = useRef<HTMLDivElement>(null);
  const entries = Object.entries(tags);
  const selected = entries.find(([key]) => key === selectedKey);

  // An open list owns the pointer and the keyboard, so both listeners sit
  // on the document: the press that dismisses it lands anywhere on the
  // page, and the page's one-key shortcuts must not fire underneath it.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      if (!group.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
      e.stopPropagation();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  return (
    <div className="tag-select" ref={group}>
      <button
        className="revision-select tag-select-head"
        aria-label="Tag"
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Graph the changes that share this tag's value"
        disabled={selected === undefined}
        onClick={() => {
          setOpen((v) => !v);
        }}
      >
        {selected === undefined ? (
          <span className="tag-select-key">no tags</span>
        ) : (
          <TagRow tagKey={selected[0]} value={selected[1]} />
        )}
        <svg className="tag-select-caret" viewBox="0 0 10 6" aria-hidden="true">
          <path
            d="M1 1 5 5 9 1"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
          />
        </svg>
      </button>
      {open ? (
        <div className="tag-select-list" role="listbox">
          {entries.map(([key, value]) => (
            <button
              key={key}
              className="revision-select"
              role="option"
              aria-selected={key === selectedKey}
              onClick={() => {
                onSelectKey(key);
                setOpen(false);
              }}
            >
              <TagRow tagKey={key} value={value} />
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function TagRow({ tagKey, value }: { tagKey: string; value: string }) {
  return (
    <>
      <span className="tag-select-key">{tagKey}</span>
      <span className="tag-select-value">{value}</span>
    </>
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
