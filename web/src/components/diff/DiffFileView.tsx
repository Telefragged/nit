import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Fragment, useEffect, useMemo, useState } from "react";
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

/** True clicks place line comments; mouseups that end a text selection
 * must not (the selection is the anchor being built — lib/selection). */
const selectionInProgress = () =>
  document.getSelection()?.isCollapsed === false;

function HunkSeparator({
  prev,
  hunk,
  colSpan,
}: {
  prev: Hunk | undefined;
  hunk: Hunk;
  colSpan: number;
}) {
  const skipped = skippedBefore(prev, hunk);
  return (
    <tr className="hunk-row">
      <td colSpan={colSpan}>
        <span className="hunk-skip">
          {skipped > 0 ? `⋯ ${skipped} unchanged lines` : "⋯"}
        </span>
        <span className="hunk-header">
          @@ -{hunk.old_start},{hunk.old_lines} +{hunk.new_start},
          {hunk.new_lines} @@ {hunk.header}
        </span>
      </td>
    </tr>
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

  /** Click handler placing a line comment, selection-aware. A click on
   * the open editor's own line is a no-op — its rangeless target must
   * not overwrite an anchored range; only the c shortcut re-anchors (it
   * carries the authoritative current selection). */
  const guardedClick = (target: DraftTarget | null) =>
    target
      ? () => {
          if (selectionInProgress()) return;
          if (
            ctx.editingTarget &&
            targetAt(ctx.editingTarget, target.file, target.side, target.line)
          ) {
            return;
          }
          ctx.setEditingTarget(target);
        }
      : undefined;

  // In split layout, the side the current mouse drag started on; the
  // other side is made unselectable (styles.css `sel-old`/`sel-new`) so a
  // cross-column drag yields one side's text, gerrit-style. Cleared when
  // the drag ends — user-select only gates *making* selections, so the
  // completed one (which c consumes) survives; leaving the lock in place
  // would silently exclude the column from selections that do not start
  // on this table (Ctrl+A, find-and-select).
  const [selSide, setSelSide] = useState<CommentSide | null>(null);
  useEffect(() => {
    if (selSide === null) return undefined;
    const clear = () => setSelSide(null);
    document.addEventListener("mouseup", clear);
    return () => document.removeEventListener("mouseup", clear);
  }, [selSide]);

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

  /** Comment anchor a click on this line produces, or null if forbidden. */
  function targetFor(line: Line): DraftTarget | null {
    if (
      line.kind === "del" ||
      (line.kind === "context" && line.new === undefined)
    ) {
      if (ctx.interdiff) return null; // old side of an interdiff: unsupported
      if (line.old === undefined) return null;
      return { file: file.path, side: "old", line: line.old };
    }
    if (line.new === undefined) return null;
    return { file: file.path, side: "new", line: line.new };
  }

  /** Thread/editor rows attached under a diff line. A context line owns
   * anchors on both sides; add/del lines own exactly one. */
  function metaRows(line: Line, colSpan: number) {
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
          <tr className="meta-row" key={`t-${side}-${t.root.id}`}>
            <td colSpan={colSpan}>
              <CommentThread thread={t} changeId={ctx.changeId} />
            </td>
          </tr>,
        );
      }
      if (
        ctx.editingTarget &&
        targetAt(ctx.editingTarget, file.path, side, no)
      ) {
        rows.push(
          <tr className="meta-row" key={`editor-${side}-${no}`}>
            <td colSpan={colSpan}>
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
            </td>
          </tr>,
        );
      }
    }
    return rows;
  }

  function unifiedRows(hunk: Hunk) {
    return hunk.lines.map((line, li) => {
      const target = targetFor(line);
      return (
        <Fragment key={li}>
          <tr
            className={`line-row ${line.kind} ${target ? "clickable" : ""}`}
            onClick={guardedClick(target)}
            title={target ? "Comment on this line" : undefined}
          >
            <td className="g">{line.old ?? ""}</td>
            <td className="g">{line.new ?? ""}</td>
            <td className="code" data-old={line.old} data-new={line.new}>
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
            </td>
          </tr>
          {metaRows(line, 3)}
        </Fragment>
      );
    });
  }

  function splitRows(hunk: Hunk) {
    return pairLines(hunk.lines).map((pair, pi) => {
      const leftTarget = pair.left
        ? targetFor({ ...pair.left, new: undefined })
        : null;
      const rightTarget =
        pair.right && pair.right.new !== undefined
          ? { file: file.path, side: "new" as const, line: pair.right.new }
          : null;
      return (
        <Fragment key={pi}>
          <tr className="line-row split">
            <td
              className={`g ${pair.left ? pair.left.kind : "void"}`}
              data-side="old"
              onClick={guardedClick(leftTarget)}
            >
              {pair.left?.old ?? ""}
            </td>
            <td
              className={`code half ${pair.left ? pair.left.kind : "void"} ${leftTarget ? "clickable" : ""}`}
              data-side="old"
              data-old={pair.left?.old}
              onClick={guardedClick(leftTarget)}
            >
              {pair.left ? (
                <Code
                  text={pair.left.text}
                  lang={lang}
                  mark={marks.get(pair.left)}
                  rangeMarks={cellRangeMarks(pair.left, ["old"])}
                  className="code-text"
                />
              ) : null}
            </td>
            <td
              className={`g ${pair.right ? pair.right.kind : "void"}`}
              data-side="new"
              onClick={guardedClick(rightTarget)}
            >
              {pair.right?.new ?? ""}
            </td>
            <td
              className={`code half ${pair.right ? pair.right.kind : "void"} ${rightTarget ? "clickable" : ""}`}
              data-side="new"
              data-new={pair.right?.new}
              onClick={guardedClick(rightTarget)}
            >
              {pair.right ? (
                <Code
                  text={pair.right.text}
                  lang={lang}
                  mark={marks.get(pair.right)}
                  rangeMarks={cellRangeMarks(pair.right, ["new"])}
                  className="code-text"
                />
              ) : null}
            </td>
          </tr>
          {pair.left && pair.left.kind !== "context"
            ? metaRows(pair.left, 4)
            : null}
          {pair.right ? metaRows(pair.right, 4) : null}
        </Fragment>
      );
    });
  }

  const colSpan = layout === "unified" ? 3 : 4;
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
            <table
              className={`diff-table ${
                layout === "split" && selSide ? `sel-${selSide}` : ""
              }`}
              onMouseDown={
                layout === "split"
                  ? (e) =>
                      setSelSide(
                        ((e.target as Element)
                          .closest("[data-side]")
                          ?.getAttribute("data-side") ??
                          null) as CommentSide | null,
                      )
                  : undefined
              }
            >
              {file.hunks.map((hunk, hi) => (
                <tbody key={hi}>
                  <HunkSeparator
                    prev={file.hunks[hi - 1]}
                    hunk={hunk}
                    colSpan={colSpan}
                  />
                  {layout === "unified" ? unifiedRows(hunk) : splitRows(hunk)}
                </tbody>
              ))}
            </table>
          )}
        </>
      )}
    </section>
  );
}
