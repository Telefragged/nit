import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Fragment, type MouseEvent as ReactMouseEvent, useMemo } from "react";
import { createDraft } from "../../api/client";
import type {
  CommentRange,
  CommentSide,
  DiffFile,
  Hunk,
  Line,
} from "../../api/types";
import type { IntralineRange } from "../../lib/diffview";
import {
  displayPath,
  intralineMarks,
  pairLines,
  rangeSliceOnLine,
  skippedBefore,
  statusLetter,
} from "../../lib/diffview";
import {
  highlightLine,
  languageFor,
  markIntraline,
  markTextRange,
} from "../../lib/highlight";
import type { DraftTarget } from "../../pages/reviewContext";
import { useReview } from "../../pages/reviewContext";
import CommentEditor from "../CommentEditor";
import CommentThread, { type Thread } from "../CommentThread";

/** A commented char window to tint on one line; `active` is the open
 * editor's pending selection (brighter chrome). */
interface RangeMark {
  from: number;
  to: number;
  active: boolean;
}

function Code({
  text,
  lang,
  mark,
  rangeMarks,
  className,
}: {
  text: string;
  lang: string | null;
  mark?: IntralineRange;
  /** Comment-range tints; overlaps stack (nested spans layer the rgba). */
  rangeMarks?: RangeMark[];
  /** `code-text` on diff cells — the selection contract (lib/selection). */
  className?: string;
}) {
  const html = useMemo(() => {
    let h = highlightLine(text, lang);
    if (mark) h = markIntraline(h, mark[0], mark[1]);
    for (const r of rangeMarks ?? []) {
      h = markTextRange(
        h,
        r.from,
        r.to,
        r.active ? "comment-range comment-range-active" : "comment-range",
      );
    }
    return h;
  }, [text, lang, mark, rangeMarks]);
  // Highlight.js escapes its input; nothing user-controlled is injected raw.
  return (
    <span
      className={className}
      dangerouslySetInnerHTML={{ __html: html || "​" }}
    />
  );
}

const anchorLine = (t: Thread) => t.root.rendered_line ?? t.root.line;
const targetAt = (a: DraftTarget, file: string, side: string, line: number) =>
  a.file === file && a.side === side && a.line === line;

function HunkSeparator({ prev, hunk }: { prev: Hunk | undefined; hunk: Hunk }) {
  const skipped = skippedBefore(prev, hunk);
  return (
    <div className="hunk-row">
      <span className="hunk-skip">
        {skipped > 0 ? `⋯ ${skipped} unchanged lines` : "⋯"}
      </span>
      <span className="hunk-header">
        @@ -{hunk.old_start},{hunk.old_lines} +{hunk.new_start},{hunk.new_lines}{" "}
        @@ {hunk.header}
      </span>
    </div>
  );
}

/** One file section: header, outdated/unanchored threads, hunks with inline
 * threads and the draft editor. Collapsible: when collapsed only the header
 * row renders (inline threads included — the rail's counts still signal
 * them); the header click toggles. */
export default function DiffFileView({
  file,
  layout,
  threads,
  domId,
  collapsed,
  onToggle,
}: {
  file: DiffFile;
  layout: "unified" | "split";
  threads: Thread[];
  domId: string;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const ctx = useReview();
  const queryClient = useQueryClient();
  const lang = languageFor(file.path);

  // Intraline emphasis for modified line pairs, per hunk (keyed by line
  // object identity, so unified and split rows share the same map).
  const marks = useMemo(() => {
    const map = new Map<Line, IntralineRange>();
    for (const hunk of file.hunks) {
      for (const [line, range] of intralineMarks(hunk.lines)) {
        map.set(line, range);
      }
    }
    return map;
  }, [file]);

  // Anchors actually present in this diff, per side.
  const present = useMemo(() => {
    const set = new Set<string>();
    for (const hunk of file.hunks) {
      for (const line of hunk.lines) {
        if (line.old !== undefined) set.add(`old:${line.old}`);
        if (line.new !== undefined) set.add(`new:${line.new}`);
      }
    }
    return set;
  }, [file]);

  const topThreads: Thread[] = [];
  const inline = new Map<string, Thread[]>();
  for (const t of threads) {
    const line = anchorLine(t);
    const key = `${t.root.side}:${line}`;
    if (t.root.outdated || line === null || !present.has(key)) {
      topThreads.push(t);
    } else {
      const list = inline.get(key) ?? [];
      list.push(t);
      inline.set(key, list);
    }
  }

  const create = useMutation({
    mutationFn: (input: { target: DraftTarget; body: string }) =>
      createDraft(ctx.changeId, {
        revision: ctx.draftRevision,
        file: input.target.file,
        line: input.target.line,
        side: input.target.side,
        range: input.target.range,
        body: input.body,
      }),
    onSuccess: () => {
      // The body was saved, not discarded: clear dirtiness before the
      // guarded setter closes the editor so it doesn't prompt.
      ctx.editorDirty.current = false;
      ctx.setEditingTarget(null);
      void queryClient.invalidateQueries({
        queryKey: ["change", ctx.changeId],
      });
    },
  });

  // Split layout only: while a drag is in flight, lock selection to the
  // side it started on (styles.css `sel-old`/`sel-new` make the other
  // column unselectable) so a cross-column drag yields one side's
  // contiguous text — the shape a comment range needs. Done imperatively
  // on the grid node, not via React state: a state change on mousedown
  // re-renders mid-gesture and drops the nascent selection, and the lock
  // would land too late to keep the drag on one side. Cleared on mouseup —
  // user-select only gates *making* a selection, so the finished one
  // (which c consumes) survives, and selections not started here (Ctrl+A,
  // find-and-select) are never constrained.
  const lockSelectionSide = (e: ReactMouseEvent) => {
    const side = (e.target as Element)
      .closest("[data-side]")
      ?.getAttribute("data-side");
    if (side !== "old" && side !== "new") return;
    const grid = e.currentTarget as HTMLElement;
    grid.classList.add(`sel-${side}`);
    document.addEventListener(
      "mouseup",
      () => grid.classList.remove("sel-old", "sel-new"),
      { once: true },
    );
  };

  // Selected-text ranges to tint: every inline thread's ported range,
  // plus the open editor's pending selection — its "what am I commenting
  // on" feedback once the DOM selection is dismissed.
  const rangePaints = useMemo(() => {
    const paints: {
      side: CommentSide;
      range: CommentRange;
      active: boolean;
    }[] = [];
    for (const t of threads) {
      if (!t.root.outdated && t.root.rendered_range) {
        paints.push({
          side: t.root.side,
          range: t.root.rendered_range,
          active: false,
        });
      }
    }
    const et = ctx.editingTarget;
    if (et?.range && et.file === file.path) {
      paints.push({ side: et.side, range: et.range, active: true });
    }
    return paints;
  }, [threads, ctx.editingTarget, file.path]);

  /** The comment-range tints falling on `line`'s text in a cell showing
   * the given sides (unified cells show both; split cells one). */
  function cellRangeMarks(
    line: Line,
    sides: readonly CommentSide[],
  ): RangeMark[] | undefined {
    const marks: RangeMark[] = [];
    for (const p of rangePaints) {
      if (!sides.includes(p.side)) continue;
      const no = p.side === "new" ? line.new : line.old;
      if (no === undefined) continue;
      const w = rangeSliceOnLine(p.range, no, line.text.length);
      if (w) marks.push({ from: w[0], to: w[1], active: p.active });
    }
    return marks.length > 0 ? marks : undefined;
  }

  /** Thread/editor rows attached under a diff line. A context line owns
   * anchors on both sides; add/del lines own exactly one. */
  function metaRows(line: Line) {
    const rows = [];
    const sides: Array<["old" | "new", number | undefined]> =
      line.kind === "context"
        ? [
            ["old", line.old],
            ["new", line.new],
          ]
        : line.kind === "del"
          ? [["old", line.old]]
          : [["new", line.new]];
    for (const [side, no] of sides) {
      if (no === undefined) continue;
      for (const t of inline.get(`${side}:${no}`) ?? []) {
        rows.push(
          <div className="meta-row" key={`t-${side}-${t.root.id}`}>
            <CommentThread thread={t} changeId={ctx.changeId} />
          </div>,
        );
      }
      if (
        ctx.editingTarget &&
        targetAt(ctx.editingTarget, file.path, side, no)
      ) {
        rows.push(
          <div className="meta-row" key={`editor-${side}-${no}`}>
            <CommentEditor
              saving={create.isPending}
              onSave={(body) =>
                create.mutate({ target: ctx.editingTarget!, body })
              }
              onCancel={() => ctx.setEditingTarget(null)}
              onDirtyChange={(dirty) => {
                ctx.editorDirty.current = dirty;
              }}
            />
          </div>,
        );
      }
    }
    return rows;
  }

  function unifiedRows(hunk: Hunk) {
    return hunk.lines.map((line, li) => (
      <Fragment key={li}>
        <div className="line-row">
          <span className={`g ${line.kind}`}>{line.old ?? ""}</span>
          <span className={`g ${line.kind}`}>{line.new ?? ""}</span>
          <span
            className={`code ${line.kind}`}
            data-old={line.old}
            data-new={line.new}
          >
            <span className="sign">
              {line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}
            </span>
            <Code
              text={line.text}
              lang={lang}
              mark={marks.get(line)}
              rangeMarks={cellRangeMarks(line, ["old", "new"])}
              className="code-text"
            />
          </span>
        </div>
        {metaRows(line)}
      </Fragment>
    ));
  }

  /** One side of a split row: its gutter + code-half spans. The code cell
   * carries only data-{side} (never both) so lib/selection's sideOf
   * resolves a one-column drag to this side. */
  function sideCell(line: Line | null, side: "old" | "new") {
    return (
      <>
        <span className={`g ${line ? line.kind : "void"}`} data-side={side}>
          {line?.[side] ?? ""}
        </span>
        <span
          className={`code half ${line ? line.kind : "void"}`}
          data-side={side}
          data-old={side === "old" ? line?.old : undefined}
          data-new={side === "new" ? line?.new : undefined}
        >
          {line ? (
            <Code
              text={line.text}
              lang={lang}
              mark={marks.get(line)}
              rangeMarks={cellRangeMarks(line, [side])}
              className="code-text"
            />
          ) : null}
        </span>
      </>
    );
  }

  function splitRows(hunk: Hunk) {
    return pairLines(hunk.lines).map((pair, pi) => (
      <Fragment key={pi}>
        <div className="line-row">
          {sideCell(pair.left, "old")}
          {sideCell(pair.right, "new")}
        </div>
        {pair.left && pair.left.kind !== "context" ? metaRows(pair.left) : null}
        {pair.right ? metaRows(pair.right) : null}
      </Fragment>
    ));
  }

  const letter = statusLetter(file);

  return (
    <section
      className={`file-section ${collapsed ? "collapsed" : ""}`}
      id={domId}
      data-diff-path={file.path}
    >
      <header
        className="file-header"
        onClick={onToggle}
        aria-expanded={!collapsed}
        title={collapsed ? "Expand file" : "Collapse file"}
      >
        <span className="fchevron">{collapsed ? "▸" : "▾"}</span>
        <span className={letter ? `fstat fstat-${letter}` : "fstat"}>
          {letter}
        </span>
        <span className="fpath">
          {file.old_path ? (
            <>
              <span className="old-path">{file.old_path}</span> → {file.path}
            </>
          ) : (
            displayPath(file.path)
          )}
        </span>
        <span className="spacer" />
        {file.binary ? (
          <span className="dim">binary</span>
        ) : (
          <span className="fcounts">
            <span className="plus">+{file.additions}</span>{" "}
            <span className="minus">−{file.deletions}</span>
          </span>
        )}
      </header>

      {collapsed ? null : (
        <>
          {topThreads.length > 0 ? (
            <div className="outdated-group">
              <div className="outdated-title">
                Comments not anchored in this diff
              </div>
              {topThreads.map((t) => (
                <div className="outdated-item" key={t.root.id}>
                  <div className="line-excerpt">
                    {t.root.outdated ? (
                      <span className="badge badge-amber outdated-tag">
                        OUTDATED
                      </span>
                    ) : null}
                    <span className="excerpt-line">r{t.root.revision}</span>
                    <Code
                      text={t.root.line_text ?? "(file comment)"}
                      lang={lang}
                    />
                  </div>
                  <CommentThread thread={t} changeId={ctx.changeId} />
                </div>
              ))}
            </div>
          ) : null}

          {file.binary ? (
            <div className="binary-note">Binary file — contents not shown</div>
          ) : (
            <div
              className={`diff-grid ${
                layout === "split" ? "diff-grid-split" : "diff-grid-unified"
              }`}
              onMouseDown={layout === "split" ? lockSelectionSide : undefined}
            >
              {file.hunks.map((hunk, hi) => (
                <Fragment key={hi}>
                  <HunkSeparator prev={file.hunks[hi - 1]} hunk={hunk} />
                  {layout === "unified" ? unifiedRows(hunk) : splitRows(hunk)}
                </Fragment>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}
