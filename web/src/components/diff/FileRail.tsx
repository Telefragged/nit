import type { DiffFile } from "../../api/types";
import type { Thread } from "../CommentThread";

const STATUS_LETTER: Record<DiffFile["status"], string> = {
  added: "A",
  deleted: "D",
  modified: "M",
  renamed: "R",
};

export function fileDomId(index: number): string {
  return `file-${index}`;
}

/** Left rail: every file in the diff with status letter, +/- counts and
 * comment markers. Selecting scrolls to the file section. */
export default function FileRail({
  files,
  threadsByFile,
  activeIndex,
  onSelect,
}: {
  files: DiffFile[];
  threadsByFile: Map<string, Thread[]>;
  activeIndex: number | null;
  onSelect: (index: number) => void;
}) {
  return (
    <aside className="file-rail">
      <div className="rail-title">
        {files.length} file{files.length === 1 ? "" : "s"}
      </div>
      {files.map((file, i) => {
        const threads = threadsByFile.get(file.path) ?? [];
        const drafts = threads.filter((t) => t.root.state === "draft").length;
        const published = threads.length - drafts;
        return (
          <div
            key={file.path}
            className={`rail-item ${i === activeIndex ? "active" : ""}`}
            onClick={() => onSelect(i)}
            title={
              file.old_path ? `${file.old_path} → ${file.path}` : file.path
            }
          >
            <span className={`fstat fstat-${STATUS_LETTER[file.status]}`}>
              {STATUS_LETTER[file.status]}
            </span>
            <span className="pathname">
              {/* bdi keeps the rtl ellipsis trick from reordering chars */}
              <bdi>{file.path}</bdi>
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
    </aside>
  );
}
