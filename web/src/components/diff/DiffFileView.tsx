import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Fragment,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  useMemo,
} from "react";
import { createDraft } from "../../api/client";
import type {
  CommentRange,
  CommentSide,
  DiffFile,
  Hunk,
  Line,
} from "../../api/types";
import {
  commentCountLabel,
  commentPlacement,
  draftAnchor,
} from "../../lib/comments";
import type { IntralineRange } from "../../lib/diffview";
import {
  displayPath,
  intralineMarks,
  pairLines,
  rangeSliceOnLine,
  type RowPair,
  skippedBefore,
  statusLetter,
} from "../../lib/diffview";
import {
  highlightLine,
  languageFor,
  markIntraline,
  markTextRange,
} from "../../lib/highlight";
import { selectionAnchorSide } from "../../lib/selection";
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

const targetAt = (a: DraftTarget, file: string, side: string, line: number) =>
  a.file === file && a.side === side && a.line === line;

/** Class suffix marking a rebase-drift line (docs/api.md "Rebase-aware
 * interdiffs"), so its gutter and code cell render contained. */
const driftClass = (line: Line | null) => (line?.drift ? " drift" : "");

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

/** One file section: header, off-hunk/file-level threads, hunks with inline
 * threads and the draft editor. Threads place by the diff range — new-side
 * under the right column, old-side under the left (docs/api.md "Comment
 * placement"). Collapsible: when collapsed only the header row renders
 * (inline threads included — the rail's counts still signal them); the
 * header click toggles. */
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

  // Bucket each thread by where its anchor lands in the current diff range
  // (docs/api.md "Comment placement"). A thread pinned to a revision that
  // is neither FROM nor TO is dropped — it is not part of this diff.
  // File-level comments (no line) have no column; they group at the top.
  const topThreads: Thread[] = [];
  const inline = new Map<string, Thread[]>();
  for (const t of threads) {
    if (t.root.line === null) {
      topThreads.push(t);
      continue;
    }
    const p = commentPlacement(t.root, ctx.selected, ctx.against);
    if (!p) continue;
    const key = `${p.side}:${p.line}`;
    if (present.has(key)) {
      const list = inline.get(key) ?? [];
      list.push(t);
      inline.set(key, list);
    } else {
      topThreads.push(t);
    }
  }

  const create = useMutation({
    mutationFn: (input: { target: DraftTarget; body: string }) => {
      // The visual column maps back to a stored (revision, side): the new
      // column is the selected revision; the old column is its parent
      // (base) or, in an interdiff, the FROM revision's own side.
      const anchor = draftAnchor(input.target.side, ctx.selected, ctx.against);
      return createDraft(ctx.changeId, {
        revision: anchor.revision,
        file: input.target.file,
        line: input.target.line,
        side: anchor.side,
        range: input.target.range,
        body: input.body,
      });
    },
    onSuccess: () => {
      // The body was saved, not discarded: clear dirtiness before the
      // guarded setter closes the editor so it doesn't prompt.
      ctx.setEditorDirty(false);
      ctx.setEditingTarget(null);
      void queryClient.invalidateQueries({
        queryKey: ["change", ctx.changeId],
      });
    },
  });

  // Split layout only: while a drag is in flight, lock selection to the
  // side it started on (styles.css `sel-old`/`sel-new` set the other column
  // user-select: none) so a cross-column drag yields one side's contiguous
  // text — the shape a comment range needs. This bounds the *captured* text
  // in engines that honor user-select across a spanning selection; the
  // cross-column *paint* is handled separately and unconditionally by the
  // diff column's data-sel-side ::selection rule (ReviewPage). Done
  // imperatively on the grid node, not via React state: a state change on
  // mousedown re-renders mid-gesture and drops the nascent selection, and
  // the lock would land too late to keep the drag on one side. Cleared on
  // mouseup so the finished selection (which c consumes) survives and later
  // selections (Ctrl+A, find-and-select) are never constrained.
  const lockSelectionSide = (e: ReactMouseEvent) => {
    const side = selectionAnchorSide(e.target as Node);
    if (side === null) return;
    const grid = e.currentTarget as HTMLElement;
    grid.classList.add(`sel-${side}`);
    document.addEventListener(
      "mouseup",
      () => {
        grid.classList.remove("sel-old", "sel-new");
      },
      { once: true },
    );
  };

  // Selected-text ranges to tint: every placed thread's range painted on
  // the column it lands in, plus the open editor's pending selection — its
  // "what am I commenting on" feedback once the DOM selection is dismissed.
  const rangePaints = useMemo(() => {
    const paints: {
      side: CommentSide;
      range: CommentRange;
      active: boolean;
    }[] = [];
    for (const t of threads) {
      if (!t.root.range) continue;
      const p = commentPlacement(t.root, ctx.selected, ctx.against);
      if (p) paints.push({ side: p.side, range: t.root.range, active: false });
    }
    const et = ctx.editingTarget;
    if (et?.range && et.file === file.path) {
      paints.push({ side: et.side, range: et.range, active: true });
    }
    return paints;
  }, [threads, ctx.editingTarget, ctx.selected, ctx.against, file.path]);

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

  /** The thread + editor items anchored at one (side, line) cell — bare,
   * so unified and split can lay them out differently. */
  function metaItems(side: "old" | "new", no: number | undefined): ReactNode[] {
    if (no === undefined) return [];
    const items: ReactNode[] = [];
    for (const t of inline.get(`${side}:${no}`) ?? []) {
      items.push(
        <div className="meta-item" key={`t-${side}-${t.root.id}`}>
          <CommentThread thread={t} changeId={ctx.changeId} />
        </div>,
      );
    }
    if (ctx.editingTarget && targetAt(ctx.editingTarget, file.path, side, no)) {
      const target = ctx.editingTarget;
      items.push(
        <div className="meta-item" key={`editor-${side}-${no}`}>
          <CommentEditor
            saving={create.isPending}
            onSave={(body) => {
              create.mutate({ target, body });
            }}
            onCancel={() => ctx.setEditingTarget(null)}
            onDirtyChange={(dirty) => {
              ctx.setEditorDirty(dirty);
            }}
          />
        </div>,
      );
    }
    return items;
  }

  /** Unified meta row: a line owns both sides (context) or one (add/del);
   * all its items stack in one full-width row below it. */
  function unifiedMeta(line: Line): ReactNode {
    const items =
      line.kind === "context"
        ? [...metaItems("old", line.old), ...metaItems("new", line.new)]
        : line.kind === "del"
          ? metaItems("old", line.old)
          : metaItems("new", line.new);
    return items.length > 0 ? <div className="meta-row">{items}</div> : null;
  }

  /** Split meta row: old-side items go under the left column, new-side
   * under the right — each pinned to that side (docs/api.md placement). */
  function splitMeta(pair: RowPair): ReactNode {
    const left = metaItems("old", pair.left?.old);
    const right = metaItems("new", pair.right?.new);
    if (left.length === 0 && right.length === 0) return null;
    return (
      <div className="meta-row meta-split">
        <div className="meta-col meta-col-old">{left}</div>
        <div className="meta-col meta-col-new">{right}</div>
      </div>
    );
  }

  function unifiedRows(hunk: Hunk) {
    return hunk.lines.map((line, li) => (
      <Fragment key={li}>
        <div className="line-row">
          <span className={`g ${line.kind}${driftClass(line)}`}>
            {line.old ?? ""}
          </span>
          <span className={`g ${line.kind}${driftClass(line)}`}>
            {line.new ?? ""}
          </span>
          <span
            className={`code ${line.kind}${driftClass(line)}`}
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
        {unifiedMeta(line)}
      </Fragment>
    ));
  }

  /** One side of a split row: its gutter + code-half spans. The code cell
   * carries only data-{side} (never both) so lib/selection's sideOf
   * resolves a one-column drag to this side. */
  function sideCell(line: Line | null, side: "old" | "new") {
    return (
      <>
        <span
          className={`g ${line ? line.kind : "void"}${driftClass(line)}`}
          data-side={side}
        >
          {line?.[side] ?? ""}
        </span>
        <span
          className={`code half ${line ? line.kind : "void"}${driftClass(line)}`}
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
        {splitMeta(pair)}
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
        role="button"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
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
        {/* Threads visible in the current range for this file: the `threads`
            prop is already range-filtered (docs/api.md "Comment placement"),
            so a thread pinned to a hidden revision is not counted. */}
        {threads.length > 0 ? (
          <span className="fcomments">{commentCountLabel(threads.length)}</span>
        ) : null}
      </header>

      {collapsed ? null : (
        <>
          {topThreads.length > 0 ? (
            <div className="outdated-group">
              <div className="outdated-title">Comments not on a shown line</div>
              {topThreads.map((t) => (
                <div className="outdated-item" key={t.root.id}>
                  <div className="line-excerpt">
                    <span className="excerpt-line">
                      r{t.root.revision}
                      {/* Label the column it renders under (placement side),
                          not the raw stored side — an interdiff-left thread
                          is stored "new" on the FROM revision. */}
                      {t.root.line !== null
                        ? ` · ${commentPlacement(t.root, ctx.selected, ctx.against)?.side ?? t.root.side}`
                        : ""}
                    </span>
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
