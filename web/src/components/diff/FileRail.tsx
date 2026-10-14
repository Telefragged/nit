import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import {
  ChevronRight,
  File,
  FileMinus,
  FilePlus,
  Folder,
  MessageSquare,
  type LucideIcon,
} from "lucide-react";
import { Link } from "react-router-dom";
import type { DiffFile } from "../../api/types";
import { COMMIT_MSG_PATH } from "../../api/types";
import { anchorAt, shownAt, type UiThread } from "../../lib/comments";
import {
  diffTotals,
  fileStatus,
  splitExtension,
  statusLetter,
  treePaths,
} from "../../lib/diffview";
import { buildTree, type TreeNode } from "../../lib/fileTree";
import { REVEAL, useReview } from "../../pages/reviewContext";

/** Where in its file the diff shows a thread. */
function threadLines(t: UiThread): string {
  const at = anchorAt(shownAt(t).anchor);
  if (at === null) return "file";
  if ("whole" in at) return `line ${at.whole}`;
  const { start_line, start_char, end_line, end_char } = at.selection;
  return `range ${start_line}:${start_char}-${end_line}:${end_char}`;
}

/** The row's trailing lane: draft and published comment counts, the +/-
 * churn, then the status letter. Binary files carry no churn. */
function Counts({
  file,
  drafts,
  published,
}: {
  file: DiffFile;
  drafts: number;
  published: number;
}) {
  return (
    <span className="rail-counts">
      {drafts > 0 ? <span className="rail-drafts">{drafts}d</span> : null}
      {published > 0 ? <span>{published}c</span> : null}
      {file.binary ? (
        <span className="dim">bin</span>
      ) : (
        <>
          <span className="plus">+{file.additions}</span>
          <span className="minus">−{file.deletions}</span>
        </>
      )}
      <span className="rail-status">{statusLetter(file)}</span>
    </span>
  );
}

/** The icon of a file row: an added or deleted file says so. */
const FILE_ICON: Record<ReturnType<typeof fileStatus>, LucideIcon> = {
  added: FilePlus,
  deleted: FileMinus,
  message: File,
  modified: File,
  renamed: File,
};

/** The mark of a row that holds other rows: a chevron that points right
 * when closed and down when open. */
const Chevron = ({ open }: { open: boolean }) => (
  <ChevronRight
    className={open ? "rail-chevron open" : "rail-chevron"}
    size={14}
    aria-hidden
  />
);

/** The left padding of a row `depth` levels down the tree. */
const indent = (depth: number) => ({ paddingLeft: depth * 12 + 4 });

/** `set` with `key` added, or removed when it holds it. */
function flip(set: ReadonlySet<string>, key: string): ReadonlySet<string> {
  const next = new Set(set);
  if (!next.delete(key)) next.add(key);
  return next;
}

/** Left rail: the diff as a directory tree, each file with its status, its
 * comment counts and its +/- churn. Selecting a file expands its section
 * and scrolls to it. A file with published threads opens to a row per
 * thread, and each row links to its thread. The title row toggles all
 * sections at once. A long diff needs this bulk toggle, because every
 * file starts collapsed. */
export default function FileRail({
  files,
  threadsByFile,
  activeIndex,
  onSelect,
  allExpanded,
  onToggleAll,
}: {
  files: DiffFile[];
  threadsByFile: Map<string, UiThread[]>;
  activeIndex: number | null;
  onSelect: (index: number) => void;
  allExpanded: boolean;
  onToggleAll: () => void;
}) {
  const { linkHref } = useReview();
  const message = files.findIndex((f) => f.path === COMMIT_MSG_PATH);
  const tree = useMemo(() => buildTree(treePaths(files)), [files]);
  const [closedDirs, setClosedDirs] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const [openFiles, setOpenFiles] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  // Keep the active row visible in the rail's own scrollport. A row that is
  // already visible stays put, so a click does not move the rail.
  const port = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = port.current;
    const row = el?.querySelector<HTMLElement>('[aria-current="true"]');
    if (!el || !row) return;
    if (row.offsetTop < el.scrollTop) el.scrollTop = row.offsetTop;
    const bottom = row.offsetTop + row.offsetHeight;
    if (bottom > el.scrollTop + el.clientHeight)
      el.scrollTop = bottom - el.clientHeight;
  }, [activeIndex]);

  const fileRows = (index: number, name: string, depth: number) => {
    const file = files[index];
    if (!file) return null;
    const threads = threadsByFile.get(file.path) ?? [];
    const published = threads.filter(
      (t): t is UiThread & { id: number } => t.id !== null,
    );
    const open = openFiles.has(file.path);
    const FileIcon = FILE_ICON[fileStatus(file)];
    const [stem, ext] = splitExtension(name);
    return (
      <li key={file.path}>
        <div
          className={`rail-row rail-file rail-${fileStatus(file)}`}
          data-path={file.path}
          aria-current={index === activeIndex ? "true" : undefined}
          title={file.old_path ? `${file.old_path} → ${file.path}` : undefined}
          style={indent(depth)}
        >
          {published.length > 0 ? (
            <button
              className="rail-toggle rail-toggle-chevron"
              aria-expanded={open}
              aria-label={`comments on ${name}`}
              onClick={() => {
                setOpenFiles((cur) => flip(cur, file.path));
              }}
            >
              <Chevron open={open} />
            </button>
          ) : (
            <span className="rail-toggle" />
          )}
          <FileIcon className="rail-icon" size={14} aria-hidden />
          <button
            className="rail-name"
            onClick={() => {
              onSelect(index);
            }}
          >
            <span
              className="rail-stem"
              style={{ "--ext": ext.length } as CSSProperties}
            >
              {stem}
            </span>
            {ext}
          </button>
          <Counts
            file={file}
            drafts={threads.length - published.length}
            published={published.length}
          />
        </div>
        {open ? (
          <ul>
            {published.map((t) => (
              <li key={t.id}>
                <Link
                  className="rail-row rail-thread"
                  to={linkHref({ thread: t.id })}
                  state={REVEAL}
                  style={indent(depth + 1)}
                >
                  <span className="rail-toggle" />
                  <MessageSquare className="rail-icon" size={14} aria-hidden />
                  <span className="rail-name">{threadLines(t)}</span>
                  <span className="rail-counts">r{shownAt(t).revision}</span>
                </Link>
              </li>
            ))}
          </ul>
        ) : null}
      </li>
    );
  };

  const nodeRows = (node: TreeNode, depth: number): ReactNode => {
    if (node.kind === "file") return fileRows(node.index, node.name, depth);
    const open = !closedDirs.has(node.path);
    return (
      <li key={node.path}>
        <button
          className="rail-row rail-dir"
          aria-expanded={open}
          style={indent(depth)}
          onClick={() => {
            setClosedDirs((cur) => flip(cur, node.path));
          }}
        >
          <span className="rail-toggle rail-toggle-chevron">
            <Chevron open={open} />
          </span>
          <Folder className="rail-icon" size={14} aria-hidden />
          {node.name.replaceAll("/", " / ")}
        </button>
        {open ? (
          <ul>{node.children.map((c) => nodeRows(c, depth + 1))}</ul>
        ) : null}
      </li>
    );
  };

  const totals = diffTotals(files);
  return (
    <>
      <div className="rail-title">
        <span>
          {totals.count} file{totals.count === 1 ? "" : "s"}
          {/* Diff still loading (or empty): no sums to summarize. */}
          {files.length > 0 ? (
            <span className="rail-total">
              <span className="plus">+{totals.additions}</span>{" "}
              <span className="minus">−{totals.deletions}</span>
            </span>
          ) : null}
        </span>
        {files.length > 0 ? (
          <button className="linkish rail-toggle-all" onClick={onToggleAll}>
            {allExpanded ? "collapse all" : "expand all"}
          </button>
        ) : null}
      </div>
      <div className="rail-tree" ref={port}>
        <ul>
          {message === -1 ? null : fileRows(message, "Commit message", 0)}
          {tree.map((n) => nodeRows(n, 0))}
        </ul>
      </div>
    </>
  );
}
