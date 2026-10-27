import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Fragment,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createDraft } from "../../api/client";
import {
  type CommentRange,
  type Hunk,
  type Line,
  type Side,
} from "../../api/types";
import {
  anchorKind,
  anchorLineText,
  anchorRange,
  anchorSide,
  commentCountLabel,
  commentPlacement,
  draftAnchor,
  shownAt,
  threadKey,
  type UiThread,
} from "../../lib/comments";
import {
  clickLink,
  linksLine,
  sameLink,
  type FileLink,
  type LinkSpan,
} from "../../lib/diffLink";
import type { IntralineRange } from "../../lib/diffview";
import {
  displayPath,
  intralineMarks,
  oneSided,
  pairLines,
  rangeSliceOnLine,
  type RowPair,
  skipped,
  statusLetter,
} from "../../lib/diffview";
import type { ReviewFile } from "../../lib/attachments";
import { highlight, languageFor, markTextRange } from "../../lib/highlight";
import { renderedKind } from "../../lib/rendered";
import { scrollBehavior } from "../../lib/scrollspy";
import { plainClick } from "../../lib/shortcutKey";
import { selectionAnchorSide } from "../../lib/selection";
import { EXPAND_STEP, useHunkExpansion } from "../../lib/useHunkExpansion";
import {
  targetLine,
  targetRange,
  type DraftTarget,
} from "../../pages/reviewContext";
import { useReview } from "../../pages/reviewContext";
import CommentEditor from "../CommentEditor";
import CommentThread from "../CommentThread";
import Seg, { type SegOption } from "../Seg";
import RenderedView from "./RenderedView";
import StatusMark from "./StatusMark";

/** A commented char window to tint on one line; `active` (brighter chrome)
 * marks the open editor's pending selection or the hovered thread's range. */
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

/** How a file section shows a file that renders: as lines, or rendered. */
type View = "raw" | "rendered";

const VIEW_OPTIONS: SegOption<View>[] = [
  { value: "raw", label: "Raw", title: "The lines of the diff" },
  { value: "rendered", label: "Rendered", title: "The whole file, rendered" },
];

const targetAt = (a: DraftTarget, file: string, side: string, line: number) =>
  a.file === file && a.side === side && targetLine(a) === line;

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

/** A separator over a gap of `more` lines the diff does not show, shown
 * only while the gap
 * remains (a fully-revealed gap leaves the hunks contiguous, so it
 * vanishes).
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
      <span className="hunk-skip">⋯ {more} lines</span>
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
 * header click toggles.
 *
 * A line number links to its line. A shift-click on a line number extends
 * the linked span. `reveal` is the link that the section scrolls to: its
 * first linked line, or the section itself when the link names no lines.
 * The section calls `onRevealed` after it scrolls. */
export default function DiffFileView({
  file,
  layout,
  threads,
  domId,
  collapsed,
  onToggle,
  reveal,
  onRevealed,
}: {
  file: ReviewFile;
  layout: "unified" | "split";
  threads: UiThread[];
  domId: string;
  collapsed: boolean;
  onToggle: () => void;
  reveal: FileLink | null;
  onRevealed: () => void;
}) {
  const ctx = useReview();
  const queryClient = useQueryClient();
  const lang = languageFor(file.path);

  // The inline thread under the pointer, by `threadKey`. `line` is its
  // `side:line` when it anchors a whole line; a selection thread tints its
  // range instead.
  const [hovered, setHovered] = useState<{
    thread: string;
    line: string | null;
  } | null>(null);

  const expansion = useHunkExpansion(file, ctx);
  const { hunks } = expansion;

  const kind = renderedKind(file);
  const [view, setView] = useState<View>(file.attachment ? "rendered" : "raw");
  // Only the raw view shows the line that a link names.
  if (view === "rendered" && reveal?.lines) setView("raw");
  // A file without lines has only its rendered view.
  const showRendered = kind !== null && (file.binary || view === "rendered");

  const sectionRef = useRef<HTMLElement>(null);
  // The last reveal for which the effect opened a gap. The effect opens a
  // gap only once per reveal.
  const gapAsked = useRef<FileLink | null>(null);
  const { expandable, revealLine } = expansion;
  // A layout effect, so the scroll lands before the expanded section paints
  // at its old offset.
  useLayoutEffect(() => {
    const section = sectionRef.current;
    if (reveal === null || collapsed || section === null) return;
    const scroll = (to: Element) => {
      to.scrollIntoView({ behavior: scrollBehavior(), block: "start" });
      onRevealed();
    };
    const lines = reveal.lines;
    const line =
      lines &&
      section.querySelector(`.code[data-${lines.side}="${lines.start}"]`);
    if (line) {
      scroll(line);
    } else if (lines && expandable && gapAsked.current !== reveal) {
      // After the gap opens, the new hunks render the line. This effect
      // then runs again and scrolls to the line.
      gapAsked.current = reveal;
      void revealLine(lines.side, lines.start)
        .catch(() => false)
        .then((found) => {
          if (!found) scroll(section);
        });
    } else if (gapAsked.current !== reveal) {
      scroll(section);
    }
  }, [reveal, collapsed, onRevealed, hunks, expandable, revealLine]);

  // Side-by-side would leave one of a one-sided file's columns blank and
  // squeeze its text into the half beside it.
  const rendered = oneSided(file) ? "unified" : layout;

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

  // The lines that a thread can show under. The rendered view shows no
  // line rows.
  const present = useMemo(() => {
    const set = new Set<string>();
    for (const hunk of showRendered ? [] : hunks) {
      for (const line of hunk.lines) {
        if (line.old !== undefined) set.add(`old:${line.old}`);
        if (line.new !== undefined) set.add(`new:${line.new}`);
      }
    }
    return set;
  }, [hunks, showRendered]);

  // Bucket each thread by where its anchor lands in the current diff
  // range. A thread pinned to a revision that
  // is neither FROM nor TO is dropped — it is not part of this diff.
  // A file thread has no line, so no column can hold it: it renders
  // above the diff. A line thread whose line the shown hunks do not
  // cover renders there too, under a separate heading.
  const fileThreads: UiThread[] = [];
  // Carries the column it renders under, not the raw stored side — an
  // interdiff-left thread is stored "new" on the FROM revision.
  const displaced: { t: UiThread; side: Side }[] = [];
  const inline = new Map<string, UiThread[]>();
  for (const t of threads) {
    const at = shownAt(t);
    if (anchorKind(at.anchor) === "file") {
      fileThreads.push(t);
      continue;
    }
    const p = commentPlacement(at, ctx.selected, ctx.against);
    if (!p) {
      // Only the linked thread arrives without a column in the range.
      displaced.push({ t, side: anchorSide(at.anchor) });
      continue;
    }
    const key = `${p.side}:${p.line}`;
    if (present.has(key)) {
      const list = inline.get(key) ?? [];
      list.push(t);
      inline.set(key, list);
    } else {
      displaced.push({ t, side: p.side });
    }
  }

  const create = useMutation({
    mutationFn: (input: { target: DraftTarget; body: string }) => {
      // The visual column maps back to a stored (revision, side): the new
      // column is the selected revision; the old column is its parent
      // (base) or, in an interdiff, the FROM revision's own side.
      const anchor = draftAnchor(input.target.side, ctx.selected, ctx.against);
      return createDraft(ctx.changeNumber, {
        revision: anchor.revision,
        anchor: {
          line: {
            file: input.target.file,
            side: anchor.side,
            at: input.target.at,
          },
        },
        body: input.body,
      });
    },
    onSuccess: () => {
      // The body was saved, not discarded: clear dirtiness before the
      // guarded setter closes the editor so it doesn't prompt.
      ctx.setEditorDirty(false);
      ctx.setEditingTarget(null);
      void queryClient.invalidateQueries({
        queryKey: ["drafts", ctx.changeNumber],
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
      const at = shownAt(t);
      const range = anchorRange(at.anchor);
      if (!range) continue;
      const p = commentPlacement(at, ctx.selected, ctx.against);
      if (p)
        paints.push({
          side: p.side,
          range,
          active: threadKey(t) === hovered?.thread,
        });
    }
    const et = ctx.editingTarget;
    const selected = et && targetRange(et);
    if (selected && et.file === file.path) {
      paints.push({ side: et.side, range: selected, active: true });
    }
    return paints;
  }, [
    threads,
    hovered,
    ctx.editingTarget,
    ctx.selected,
    ctx.against,
    file.path,
  ]);

  /** The class suffix tinting a cell when it shows the hovered line. */
  const lineHover = (side: Side, no: number | undefined) =>
    no !== undefined && hovered?.line === `${side}:${no}` ? " line-hover" : "";

  // The span a drag over the line numbers marks. The page writes the span
  // to the URL only on release, so a drag does not rewrite the history
  // entry once per row. The router reads the new URL in a later render, so
  // the span stays until the link matches it. If the span cleared earlier,
  // the old link would show in between.
  const [drag, setDrag] = useState<LinkSpan | null>(null);
  if (drag && sameLink(ctx.link, drag)) setDrag(null);
  // The drag as the pointer events read it: the fixed end, and the span.
  const dragRef = useRef<{ anchor: number; span: LinkSpan } | null>(null);

  /** The class suffix marking a cell that shows a linked line. */
  const lineLinked = (side: Side, no: number | undefined) =>
    linksLine(drag ?? ctx.link, file.path, side, no) ? " linked" : "";

  /** A gutter's line number, as a link to its line. A press starts a span
   * at the line, or with shift extends the linked span. A drag over other
   * rows moves the far end of the span. */
  function lineNumber(side: Side, no: number | undefined): ReactNode {
    if (no === undefined) return "";
    return (
      <a
        className="line-link"
        href={ctx.linkHref({
          file: file.path,
          lines: { side, start: no, end: no },
        })}
        onMouseDown={(e) => {
          if (!plainClick(e)) return;
          // Keeps the browser from dragging the link or selecting text.
          e.preventDefault();
          const span = clickLink(ctx.link, file.path, side, no, e.shiftKey);
          const { start, end } = span.lines;
          dragRef.current = { anchor: start === no ? end : start, span };
          setDrag(span);
          document.addEventListener("mouseover", dragOver);
          document.addEventListener(
            "mouseup",
            () => {
              document.removeEventListener("mouseover", dragOver);
              const done = dragRef.current?.span;
              dragRef.current = null;
              if (done) ctx.selectLink(done);
            },
            { once: true },
          );
        }}
        onClick={(e) => {
          if (!plainClick(e)) return;
          e.preventDefault();
          // A click from the keyboard has no mousedown before it, so it
          // links the line here.
          if (e.detail === 0) {
            ctx.selectLink(
              clickLink(ctx.link, file.path, side, no, e.shiftKey),
            );
          }
        }}
      >
        {no}
      </a>
    );
  }

  /** Moves the dragged span's far end to the row of this file under the
   * pointer. */
  const dragOver = (e: MouseEvent) => {
    const cur = dragRef.current;
    if (cur === null || !(e.target instanceof Element)) return;
    const { side } = cur.span.lines;
    const row = e.target.closest(".line-row");
    if (row === null || !sectionRef.current?.contains(row)) return;
    const no = Number(
      row.querySelector(`.code[data-${side}]`)?.getAttribute(`data-${side}`),
    );
    if (!no) return;
    const start = Math.min(cur.anchor, no);
    const end = Math.max(cur.anchor, no);
    if (start === cur.span.lines.start && end === cur.span.lines.end) return;
    cur.span = { file: file.path, lines: { side, start, end } };
    setDrag(cur.span);
  };

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
        // The hover tint only points at the anchor; the line number beside
        // the thread already tells a keyboard user where it hangs.
        // eslint-disable-next-line jsx-a11y/no-static-element-interactions
        <div
          className="meta-item"
          key={`t-${side}-${threadKey(t)}`}
          onMouseEnter={() => {
            setHovered({
              thread: threadKey(t),
              line: anchorRange(shownAt(t).anchor) ? null : `${side}:${no}`,
            });
          }}
          onMouseLeave={() => {
            setHovered(null);
          }}
        >
          <CommentThread thread={t} />
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
    return hunk.lines.map((line, li) => {
      const cls = `${line.kind}${driftClass(line)}${
        lineHover("old", line.old) || lineHover("new", line.new)
      }${lineLinked("old", line.old) || lineLinked("new", line.new)}`;
      return (
        <Fragment key={li}>
          <div className="line-row">
            <span className={`g ${cls}`}>{lineNumber("old", line.old)}</span>
            <span className={`g ${cls}`}>{lineNumber("new", line.new)}</span>
            <span
              className={`code ${cls}`}
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
      );
    });
  }

  /** One side of a split row: its gutter + code-half spans. The code cell
   * carries only data-{side} (never both) so lib/selection's sideOf
   * resolves a one-column drag to this side. */
  function sideCell(line: Line | null, side: "old" | "new") {
    const no = line?.[side];
    const cls = `${line ? line.kind : "void"}${driftClass(line)}${lineHover(
      side,
      no,
    )}${lineLinked(side, no)}`;
    return (
      <>
        <span className={`g ${cls}`} data-side={side}>
          {lineNumber(side, no)}
        </span>
        <span
          className={`code half ${cls}`}
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
      ref={sectionRef}
      className={`file-section ${collapsed ? "collapsed" : ""}`}
      id={domId}
      data-diff-path={file.path}
    >
      <header
        className="file-header"
        role="button"
        tabIndex={0}
        onClick={(e) => {
          // A click on a link or a button in the header is the control's.
          if (!(e.target as Element).closest("a, button")) onToggle();
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
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
          <StatusMark file={file} />
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
        <a
          className="file-link"
          href={ctx.linkHref({ file: file.path, lines: null })}
          title="Link to this file"
          onClick={(e) => {
            if (!plainClick(e) || e.shiftKey) return;
            e.preventDefault();
            ctx.selectLink({ file: file.path, lines: null });
          }}
        >
          #
        </a>
        <span className="spacer" />
        {kind !== null && !file.binary ? (
          <Seg
            value={view}
            options={VIEW_OPTIONS}
            onChange={(next) => {
              // The rendered view has no lines to hold an open editor.
              if (
                next === "rendered" &&
                ctx.editingTarget?.file === file.path &&
                !ctx.setEditingTarget(null)
              )
                return;
              setView(next);
            }}
          />
        ) : null}
        {file.attachment ? (
          file.attachment.revision === ctx.selected ? null : (
            <span className="dim">attached to r{file.attachment.revision}</span>
          )
        ) : file.binary ? (
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
          {fileThreads.length > 0 ? (
            <div className="thread-group" data-threads="file">
              <div className="thread-group-title">File discussion</div>
              {fileThreads.map((t) => (
                <div className="thread-group-item" key={threadKey(t)}>
                  <CommentThread thread={t} />
                </div>
              ))}
            </div>
          ) : null}

          {displaced.length > 0 ? (
            <div className="thread-group" data-threads="displaced">
              <div className="thread-group-title">
                Comments not on a shown line
              </div>
              {displaced.map(({ t, side }) => (
                <div className="thread-group-item" key={threadKey(t)}>
                  <div className="line-excerpt">
                    <span className="excerpt-line">
                      r{t.revision} · {side}
                    </span>
                    <Code text={anchorLineText(t.anchor) ?? ""} lang={lang} />
                  </div>
                  <CommentThread thread={t} />
                </div>
              ))}
            </div>
          ) : null}

          {showRendered ? (
            <RenderedView file={file} kind={kind} />
          ) : file.binary ? (
            <div className="diff-note">Binary file — contents not shown</div>
          ) : hunks.length === 0 ? (
            <div className="diff-note">No changed lines to show</div>
          ) : (
            <div
              className={`diff-grid ${
                rendered === "split" ? "diff-grid-split" : "diff-grid-unified"
              }`}
              onMouseDown={rendered === "split" ? lockSelectionSide : undefined}
            >
              {hunks.map((hunk, hi) => (
                <Fragment key={hi}>
                  <HunkSeparator
                    more={skipped(hunks[hi - 1], hunk, file)}
                    hunk={hunk}
                    sep={hi}
                    expansion={expansion}
                  />
                  {rendered === "unified" ? unifiedRows(hunk) : splitRows(hunk)}
                </Fragment>
              ))}
              {/* The run below the last hunk reveals from its top only (no
                  hunk beneath to pull up from). Like the interior separators
                  it always renders; skipped → 0 collapses it when the last
                  hunk already reaches EOF. */}
              <HunkSeparator
                more={skipped(hunks.at(-1), undefined, file)}
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
