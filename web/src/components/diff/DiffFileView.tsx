import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Fragment,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  useMemo,
} from "react";
import { createDraft } from "../../api/client";
import {
  type CommentRange,
  type DiffFile,
  type Hunk,
  type Line,
  type Side,
} from "../../api/types";
import {
  commentCountLabel,
  commentPlacement,
  draftAnchor,
  threadKey,
  type UiThread,
} from "../../lib/comments";
import type { IntralineRange } from "../../lib/diffview";
import {
  displayPath,
  intralineMarks,
  pairLines,
  rangeSliceOnLine,
  type RowPair,
  skippedAfter,
  skippedBefore,
  statusLetter,
} from "../../lib/diffview";
import { highlight, languageFor, markTextRange } from "../../lib/highlight";
import { selectionAnchorSide } from "../../lib/selection";
import { EXPAND_STEP, useHunkExpansion } from "../../lib/useHunkExpansion";
import type { DraftTarget } from "../../pages/reviewContext";
import { useReview } from "../../pages/reviewContext";
import CommentEditor from "../CommentEditor";
import CommentThread from "../CommentThread";

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
  marks,
  rangeMarks,
  className,
}: {
  text: string;
  lang: string | null;
  /** The changed spans of a replaced line; a line can hold several. */
  marks?: IntralineRange[];
  /** Comment-range tints; overlaps stack (nested spans layer the rgba). */
  rangeMarks?: RangeMark[];
  /** `code-text` on diff cells — the selection contract (lib/selection). */
  className?: string;
}) {
  const html = useMemo(() => {
    // Line-at-a-time, so multi-line constructs (block comments) don't
    // carry across rows — accepted for v1.
    let h = highlight(text, lang);
    for (const m of marks ?? []) h = markTextRange(h, m[0], m[1], "intraline");
    for (const r of rangeMarks ?? []) {
      h = markTextRange(
        h,
        r.from,
        r.to,
        r.active ? "comment-range comment-range-active" : "comment-range",
      );
    }
    return h;
  }, [text, lang, marks, rangeMarks]);
  // React re-applies dangerouslySetInnerHTML on the prop object's identity,
  // never comparing the html: an inline literal rewrites every cell's text
  // nodes each render, dropping the reviewer's in-progress selection — the
  // input `c` reads. The html memo alone won't do: a tinted line's
  // `rangeMarks` is a fresh array per render, so it recomputes an equal
  // string, and only keying on the string keeps the object.
  const inner = useMemo(() => ({ __html: html || "​" }), [html]);
  // Highlight.js escapes its input; nothing user-controlled is injected raw.
  return <span className={className} dangerouslySetInnerHTML={inner} />;
}

const targetAt = (a: DraftTarget, file: string, side: string, line: number) =>
  a.file === file && a.side === side && a.line === line;

/** Class suffix marking a rebase-drift line, so its gutter and code cell
 * render contained. */
const driftClass = (line: Line | null) => (line?.drift ? " drift" : "");

/** One reveal button, floating over a separator; `kind` is both its class
 * suffix and which lines it takes. */
const ExpandButton = ({
  kind,
  count,
  title,
  busy,
  onClick,
}: {
  kind: "all" | "down" | "up";
  count: number;
  title: string;
  busy: boolean;
  onClick: () => void;
}) => (
  <button
    type="button"
    className={`hunk-expand expand-${kind}`}
    onClick={onClick}
    disabled={busy}
    title={title}
  >
    +{count}
  </button>
);

/** A separator over a gap of `more` unchanged lines, shown only while the gap
 * remains (a fully-revealed gap leaves the hunks contiguous, so it vanishes).
 * When the file is expandable the gap's reveal buttons float over it: the
 * whole run, then a stepped button per
 * edge it can pull from — down from `hunk`'s predecessor, up from `hunk`
 * itself. The top gap (`sep` 0) has no hunk above, the bottom gap no `hunk`
 * below (nor a `@@` header), and a gap one step covers needs no stepped
 * button at all. */
function HunkSeparator({
  more,
  hunk,
  sep,
  expansion,
}: {
  more: number;
  hunk: Hunk | undefined;
  sep: number;
  expansion: ReturnType<typeof useHunkExpansion>;
}) {
  const { expandable, expand, busyAt } = expansion;
  if (more === 0) return null;
  // The whole run folds into the hunk above — or below, for the top gap,
  // which has none.
  const end = sep > 0 ? "down" : "up";
  return (
    <div className="hunk-row">
      {expandable ? (
        <div className="hunk-expanders">
          <ExpandButton
            kind="all"
            count={more}
            title={`Show all ${more} line${more === 1 ? "" : "s"}`}
            busy={busyAt(end, sep)}
            onClick={() => void expand(end, sep, Infinity)}
          />
          {more > EXPAND_STEP ? (
            <div className="hunk-expand-steps">
              {sep > 0 ? (
                <ExpandButton
                  kind="down"
                  count={EXPAND_STEP}
                  title={`Show ${EXPAND_STEP} more lines below`}
                  busy={busyAt("down", sep)}
                  onClick={() => void expand("down", sep)}
                />
              ) : null}
              {hunk ? (
                <ExpandButton
                  kind="up"
                  count={EXPAND_STEP}
                  title={`Show ${EXPAND_STEP} more lines above`}
                  busy={busyAt("up", sep)}
                  onClick={() => void expand("up", sep)}
                />
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}
      <span className="hunk-skip">⋯ {more} unchanged lines</span>
      {hunk ? (
        <span className="hunk-header">
          @@ -{hunk.old_start},{hunk.old_lines} +{hunk.new_start},
          {hunk.new_lines} @@ {hunk.header}
        </span>
      ) : null}
    </div>
  );
}

/** One file section: header, off-hunk/file-level threads, hunks with inline
 * threads and the draft editor. Threads place by the diff range — new-side
 * under the right column, old-side under the left. Collapsible: when
 * collapsed only the header row renders
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
  threads: UiThread[];
  domId: string;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const ctx = useReview();
  const queryClient = useQueryClient();
  const lang = languageFor(file.path);

  const expansion = useHunkExpansion(file, ctx);
  const { hunks } = expansion;

  // Intraline emphasis for modified line pairs, per hunk (keyed by line
  // object identity, so unified and split rows share the same map).
  const marks = useMemo(() => {
    const map = new Map<Line, IntralineRange[]>();
    for (const hunk of hunks) {
      for (const [line, range] of intralineMarks(hunk.lines)) {
        map.set(line, range);
      }
    }
    return map;
  }, [hunks]);

  const present = useMemo(() => {
    const set = new Set<string>();
    for (const hunk of hunks) {
      for (const line of hunk.lines) {
        if (line.old !== undefined) set.add(`old:${line.old}`);
        if (line.new !== undefined) set.add(`new:${line.new}`);
      }
    }
    return set;
  }, [hunks]);

  // Bucket each thread by where its anchor lands in the current diff
  // range. A thread pinned to a revision that
  // is neither FROM nor TO is dropped — it is not part of this diff.
  // File-level comments (no line) have no column; they group at the top.
  const topThreads: UiThread[] = [];
  const inline = new Map<string, UiThread[]>();
  for (const t of threads) {
    if (t.line === null) {
      topThreads.push(t);
      continue;
    }
    const p = commentPlacement(t, ctx.selected, ctx.against);
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
        side: anchor.side,
        // line and range are mutually exclusive anchors — a range
        // anchors under its own end line.
        ...(input.target.range
          ? { range: input.target.range }
          : { line: input.target.line }),
        body: input.body,
      });
    },
    onSuccess: () => {
      // The body was saved, not discarded: clear dirtiness before the
      // guarded setter closes the editor so it doesn't prompt.
      ctx.setEditorDirty(false);
      ctx.setEditingTarget(null);
      void queryClient.invalidateQueries({
        queryKey: ["drafts", ctx.changeId],
      });
    },
  });

  // Split layout only: lock selection to the side a drag started on via
  // grid classes (diff.css sel-old/sel-new set user-select: none on the
  // other column) so cross-column drags yield one contiguous-text side —
  // the shape a comment range needs. Paint across columns is handled
  // separately by ReviewPage's ::selection rule. Imperative on the grid
  // node, not React state, since a mousedown re-render would drop the
  // nascent selection mid-gesture; cleared on mouseup so later selections
  // (Ctrl+A, find) aren't constrained.
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
      side: Side;
      range: CommentRange;
      active: boolean;
    }[] = [];
    for (const t of threads) {
      if (!t.range) continue;
      const p = commentPlacement(t, ctx.selected, ctx.against);
      if (p) paints.push({ side: p.side, range: t.range, active: false });
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
    sides: readonly Side[],
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
        <div className="meta-item" key={`t-${side}-${threadKey(t)}`}>
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

  /** Old-side items pin left, new-side right. */
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
              marks={marks.get(line)}
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
              marks={marks.get(line)}
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
            prop is already range-filtered, so a thread pinned to a hidden
            revision is not counted. */}
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
                <div className="outdated-item" key={threadKey(t)}>
                  <div className="line-excerpt">
                    <span className="excerpt-line">
                      r{t.revision}
                      {/* Label the column it renders under (placement side),
                          not the raw stored side — an interdiff-left thread
                          is stored "new" on the FROM revision. */}
                      {t.line !== null
                        ? ` · ${commentPlacement(t, ctx.selected, ctx.against)?.side ?? t.side}`
                        : ""}
                    </span>
                    <Code text={t.line_text ?? "(file comment)"} lang={lang} />
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
              {hunks.map((hunk, hi) => (
                <Fragment key={hi}>
                  <HunkSeparator
                    more={skippedBefore(hunks[hi - 1], hunk)}
                    hunk={hunk}
                    sep={hi}
                    expansion={expansion}
                  />
                  {layout === "unified" ? unifiedRows(hunk) : splitRows(hunk)}
                </Fragment>
              ))}
              {/* The run below the last hunk reveals from its top only,
                  toward new_total (no hunk beneath to pull up from). Like the
                  interior separators it always renders; skippedAfter → 0
                  collapses it when the last hunk already reaches EOF. */}
              <HunkSeparator
                more={skippedAfter(hunks.at(-1), file.new_total)}
                hunk={undefined}
                sep={hunks.length}
                expansion={expansion}
              />
            </div>
          )}
        </>
      )}
    </section>
  );
}
