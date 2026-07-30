import { useEffect, useMemo, useRef } from "react";
import { FileTree, useFileTree } from "@pierre/trees/react";
import type {
  FileTreeRowDecoration,
  FileTreeSortEntry,
  GitStatusEntry,
} from "@pierre/trees";
import type { DiffFile } from "../../api/types";
import { COMMIT_MSG_PATH } from "../../api/types";
import type { UiThread } from "../../lib/comments";
import { diffTotals, displayPath } from "../../lib/diffview";

/** The row the synthetic /COMMIT_MSG file rides as. A tree path is also its
 * own label, so the display name is the path — and a leading slash would
 * nest the whole diff under an empty root directory. */
const COMMIT_MSG_ROW = displayPath(COMMIT_MSG_PATH);

/** Directories first, then names — a copy of the package's own ordering,
 * whose comparator it does not export — plus the commit message pinned
 * above it all (gerrit-style, and it is not a file). */
function railSort(a: FileTreeSortEntry, b: FileTreeSortEntry): number {
  if (a.path === COMMIT_MSG_ROW) return -1;
  if (b.path === COMMIT_MSG_ROW) return 1;
  if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
  return a.basename.localeCompare(b.basename);
}

/** The row's trailing lane: draft and published comment counts, then the
 * +/- churn. Binary files carry no churn. */
function decorate(file: DiffFile, threads: UiThread[]): FileTreeRowDecoration {
  const drafts = threads.filter((t) => t.id === null).length;
  const published = threads.filter((t) => t.id !== null).length;
  const parts = [
    ...(drafts > 0 ? [{ text: `${drafts}d`, color: "var(--amber)" }] : []),
    ...(published > 0 ? [{ text: `${published}c` }] : []),
    ...(file.binary
      ? [{ text: "bin", color: "var(--text-faint)" }]
      : [
          { text: `+${file.additions}`, color: "var(--green)" },
          { text: `−${file.deletions}`, color: "var(--red)" },
        ]),
    // The lane collapses an ordinary space between parts, so they join on
    // a non-breaking one.
  ].map((p, i) => (i === 0 ? p : { ...p, text: `\u00a0${p.text}` }));
  return {
    text: parts.map((p) => p.text).join(""),
    // A renamed file shows only its new path; the old one is the tooltip.
    ...(file.old_path ? { title: `${file.old_path} → ${file.path}` } : {}),
    parts,
  };
}

/** Left rail: the diff as a directory tree, each file's status in the git
 * lane and its comment counts and +/- churn in the decoration lane.
 * Selecting a file expands its section and scrolls to it. The title row
 * toggles all sections at once — the only bulk affordance, which a long
 * diff needs since every file starts collapsed. */
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
  // The tree is path-keyed throughout — rows, status entries and every
  // callback — so the diff is indexed by the path each file rides under
  // once, here.
  const tree = useMemo(() => {
    const byPath = new Map<string, { file: DiffFile; index: number }>();
    const gitStatus: GitStatusEntry[] = [];
    files.forEach((file, index) => {
      const path = displayPath(file.path);
      byPath.set(path, { file, index });
      if (file.path !== COMMIT_MSG_PATH) {
        gitStatus.push({ path, status: file.status });
      }
    });
    return { byPath, gitStatus, paths: [...byPath.keys()] };
  }, [files]);

  const activePath =
    activeIndex === null ? null : (tree.paths[activeIndex] ?? null);

  // The tree captures its callbacks once, when the model is built, so they
  // reach the current props through this ref rather than a stale closure.
  // Published ahead of every effect below, which repaint against it.
  const live = useRef({ tree, threadsByFile, onSelect, activePath });
  useEffect(() => {
    live.current = { tree, threadsByFile, onSelect, activePath };
  });

  const { model } = useFileTree({
    paths: tree.paths,
    gitStatus: tree.gitStatus,
    sort: railSort,
    itemHeight: 22,
    density: 0.8,
    initialExpansion: "open",
    flattenEmptyDirectories: true,
    onSelectionChange: ([path]) => {
      // The selection pushed below echoes back here; reporting it would
      // reveal the file a second time.
      if (path === undefined || path === live.current.activePath) return;
      const hit = live.current.tree.byPath.get(path);
      if (hit) live.current.onSelect(hit.index);
    },
    renderRowDecoration: ({ row }) => {
      const hit = live.current.tree.byPath.get(row.path);
      return hit
        ? decorate(
            hit.file,
            live.current.threadsByFile.get(hit.file.path) ?? [],
          )
        : null;
    },
  });

  // A new diff: swap the rows and their status lane. The reset repaints, so
  // the decorations come with it — but it also re-expands every directory,
  // which is why comment traffic cannot ride this path.
  useEffect(() => {
    model.resetPaths(tree.paths);
    model.setGitStatus(tree.gitStatus);
  }, [model, tree]);

  // Comment counts sit in the decoration lane, which no tree state tracks,
  // so their repaint is ours to force. Rendering without the mounted host
  // would build a detached one, so between mounts (StrictMode remounts the
  // tree) leave it to the tree's own mount render.
  useEffect(() => {
    const host = model.getFileTreeContainer();
    if (host) model.render({ fileTreeContainer: host });
  }, [model, threadsByFile]);

  // Selection is the rail's active-file highlight, driven by the page's
  // scroll spy as much as by clicks here. `tree` is a dependency because
  // resetting the paths drops the selection with them.
  useEffect(() => {
    for (const path of model.getSelectedPaths()) {
      if (path !== activePath) model.getItem(path)?.deselect();
    }
    const item = activePath === null ? null : model.getItem(activePath);
    item?.select();
    // Keep it visible in the tree's own scrollport; already-visible rows
    // stay put, so clicks don't jump the rail.
    if (item) model.scrollToPath(item.getPath(), { offset: "nearest" });
  }, [model, activePath, tree]);

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
      <FileTree className="rail-tree" model={model} />
    </>
  );
}
