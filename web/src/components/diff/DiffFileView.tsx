import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Fragment, useMemo } from "react";
import { createDraft } from "../../api/client";
import type { DiffFile, Hunk, Line } from "../../api/types";
import { pairLines, skippedBefore } from "../../lib/diffview";
import { highlightLine, languageFor } from "../../lib/highlight";
import type { DraftTarget } from "../../pages/reviewContext";
import { useReview } from "../../pages/reviewContext";
import CommentEditor from "../CommentEditor";
import CommentThread, { type Thread } from "../CommentThread";

const STATUS_LETTER: Record<DiffFile["status"], string> = {
  added: "A",
  deleted: "D",
  modified: "M",
  renamed: "R",
};

function Code({ text, lang }: { text: string; lang: string | null }) {
  const html = useMemo(() => highlightLine(text, lang), [text, lang]);
  // Highlight.js escapes its input; nothing user-controlled is injected raw.
  return <span dangerouslySetInnerHTML={{ __html: html || "​" }} />;
}

const anchorLine = (t: Thread) => t.root.rendered_line ?? t.root.line;
const sameTarget = (a: DraftTarget, file: string, side: string, line: number) =>
  a.file === file && a.side === side && a.line === line;

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
 * threads and the draft editor. */
export default function DiffFileView({
  file,
  layout,
  threads,
  domId,
}: {
  file: DiffFile;
  layout: "unified" | "split";
  threads: Thread[];
  domId: string;
}) {
  const ctx = useReview();
  const queryClient = useQueryClient();
  const lang = languageFor(file.path);

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
        body: input.body,
      }),
    onSuccess: () => {
      ctx.setEditingTarget(null);
      void queryClient.invalidateQueries({
        queryKey: ["change", ctx.changeId],
      });
    },
  });

  /** Comment anchor a click on this line produces, or null if forbidden. */
  function targetFor(line: Line): DraftTarget | null {
    if (line.kind === "del" || (line.kind === "context" && line.new === undefined)) {
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
              <CommentThread
                thread={t}
                changeId={ctx.changeId}
                draftRevision={ctx.draftRevision}
              />
            </td>
          </tr>,
        );
      }
      if (
        ctx.editingTarget &&
        sameTarget(ctx.editingTarget, file.path, side, no)
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
            onClick={target ? () => ctx.setEditingTarget(target) : undefined}
            title={target ? "Comment on this line" : undefined}
          >
            <td className="g">{line.old ?? ""}</td>
            <td className="g">{line.new ?? ""}</td>
            <td className="code">
              <span className="sign">
                {line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}
              </span>
              <Code text={line.text} lang={lang} />
            </td>
          </tr>
          {metaRows(line, 3)}
        </Fragment>
      );
    });
  }

  function splitRows(hunk: Hunk) {
    return pairLines(hunk.lines).map((pair, pi) => {
      const leftTarget = pair.left ? targetFor({ ...pair.left, new: undefined }) : null;
      const rightTarget =
        pair.right && pair.right.new !== undefined
          ? { file: file.path, side: "new" as const, line: pair.right.new }
          : null;
      return (
        <Fragment key={pi}>
          <tr className="line-row split">
            <td
              className={`g ${pair.left ? pair.left.kind : "void"}`}
              onClick={
                leftTarget ? () => ctx.setEditingTarget(leftTarget) : undefined
              }
            >
              {pair.left?.old ?? ""}
            </td>
            <td
              className={`code half ${pair.left ? pair.left.kind : "void"} ${leftTarget ? "clickable" : ""}`}
              onClick={
                leftTarget ? () => ctx.setEditingTarget(leftTarget) : undefined
              }
            >
              {pair.left ? <Code text={pair.left.text} lang={lang} /> : null}
            </td>
            <td
              className={`g ${pair.right ? pair.right.kind : "void"}`}
              onClick={
                rightTarget
                  ? () => ctx.setEditingTarget(rightTarget)
                  : undefined
              }
            >
              {pair.right?.new ?? ""}
            </td>
            <td
              className={`code half ${pair.right ? pair.right.kind : "void"} ${rightTarget ? "clickable" : ""}`}
              onClick={
                rightTarget
                  ? () => ctx.setEditingTarget(rightTarget)
                  : undefined
              }
            >
              {pair.right ? <Code text={pair.right.text} lang={lang} /> : null}
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

  return (
    <section className="file-section" id={domId}>
      <header className="file-header">
        <span className={`fstat fstat-${STATUS_LETTER[file.status]}`}>
          {STATUS_LETTER[file.status]}
        </span>
        <span className="fpath">
          {file.old_path ? (
            <>
              <span className="old-path">{file.old_path}</span> → {file.path}
            </>
          ) : (
            file.path
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
                <Code text={t.root.line_text ?? "(file comment)"} lang={lang} />
              </div>
              <CommentThread
                thread={t}
                changeId={ctx.changeId}
                draftRevision={ctx.draftRevision}
              />
            </div>
          ))}
        </div>
      ) : null}

      {file.binary ? (
        <div className="binary-note">Binary file — contents not shown</div>
      ) : (
        <table className="diff-table">
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
    </section>
  );
}
