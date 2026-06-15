import { useEffect, useRef } from "react";
import type { DiffFile } from "../../api/types";
import type { UiThread } from "../../lib/comments";
import { diffTotals, displayPath, statusLetter } from "../../lib/diffview";

/** Left rail: every file in the diff with status letter, +/- counts and
 * comment markers. Selecting expands the file section and scrolls to it.
 * The title row totals the diff (count and +/- sums, /COMMIT_MSG excluded
 * — `diffTotals`; the rail itself still lists the commit-message entry)
 * and toggles all sections at once (the only bulk affordance — with every
 * file collapsed by default a long diff needs one). */
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
  // The rail has its own scrollport (max-height + overflow-y); when the
  // scroll spy moves the highlight, keep it visible. block:'nearest' is a
  // no-op while the item is already in view, so clicks don't jump the rail.
  const railRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (activeIndex === null) return;
    // Index access (not .item(), which the DOM lib types non-null) so an
    // out-of-range active index reads undefined and the scroll is skipped.
    railRef.current
      ?.querySelectorAll(".rail-item")
      [activeIndex]?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);
  const totals = diffTotals(files);
  return (
    <div className="file-rail" ref={railRef}>
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
      {files.map((file, i) => {
        const threads = threadsByFile.get(file.path) ?? [];
        const drafts = threads.filter((t) => t.id === null).length;
        const published = threads.filter((t) => t.id !== null).length;
        const letter = statusLetter(file);
        return (
          <div
            key={file.path}
            className={`rail-item ${i === activeIndex ? "active" : ""}`}
            role="button"
            tabIndex={0}
            onClick={() => {
              onSelect(i);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(i);
              }
            }}
            title={
              file.old_path
                ? `${file.old_path} → ${file.path}`
                : displayPath(file.path)
            }
          >
            <span className={letter ? `fstat fstat-${letter}` : "fstat"}>
              {letter}
            </span>
            <span className="pathname">
              {/* bdi keeps the rtl ellipsis trick from reordering chars */}
              <bdi>{displayPath(file.path)}</bdi>
            </span>
            {drafts > 0 ? (
              <span className="rail-counts draft-count">{drafts}d</span>
            ) : null}
            {published > 0 ? (
              <span className="rail-counts">{published}c</span>
            ) : null}
            <span className="rail-counts">
              {file.binary ? (
                <span className="dim">bin</span>
              ) : (
                <>
                  <span className="plus">+{file.additions}</span>{" "}
                  <span className="minus">−{file.deletions}</span>
                </>
              )}
            </span>
          </div>
        );
      })}
    </div>
  );
}
