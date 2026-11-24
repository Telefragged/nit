import { useQuery } from "@tanstack/react-query";
import { useContext, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import type { LineRange, Reference } from "../../api/document.gen";
import type { ChangeDetail, Draft } from "../../api/types";
import type { ReviewFile } from "../../lib/attachments";
import { threadsByFile, type UiThread } from "../../lib/comments";
import { displayPath, fileAt } from "../../lib/diffview";
import { DocumentLinkContext } from "../../lib/documentLink";
import { changeHref, rangeFile, threadsOnLines } from "../../lib/reference";
import {
  ReviewContext,
  type ReviewCtx,
  useReview,
} from "../../lib/reviewContext";
import { useDiff } from "../../lib/useDiff";
import { draftsQuery } from "../../lib/useDrafts";
import { useDraftEditor } from "../../lib/useDraftEditor";
import { fileLinesQuery } from "../../lib/useHunkExpansion";
import { useThreads } from "../../lib/useThreads";
import { baseVersion, isBase, revisionVersion } from "../../lib/version";
import DiffColumn from "../diff/DiffColumn";
import DiffFileView from "../diff/DiffFileView";
import { FileLoadNote } from "../diff/RenderedView";

type FileOf = Extract<Reference, { type: "file" }>;

const NO_DRAFTS: Draft[] = [];
const NO_THREADS: UiThread[] = [];

function WholeFile({
  file,
  threads,
  domId,
}: {
  file: ReviewFile;
  threads: UiThread[];
  domId: string;
}) {
  const { editingTarget, setEditingTarget } = useReview();
  const [collapsed, setCollapsed] = useState(false);
  return (
    <DiffFileView
      file={file}
      layout="unified"
      threads={threads}
      domId={domId}
      collapsed={collapsed}
      onToggle={() => {
        if (!collapsed && editingTarget && !setEditingTarget(null)) return;
        setCollapsed(!collapsed);
      }}
    />
  );
}

function FileRange({
  file,
  side,
  start,
  end,
  threads,
  domId,
}: LineRange & {
  file: ReviewFile;
  threads: UiThread[];
  domId: string;
}) {
  const ctx = useReview();
  const whole = useQuery(fileLinesQuery(ctx, file));
  const cut = useMemo(
    () => whole.data && rangeFile(file, whole.data.lines, { side, start, end }),
    [file, whole.data, side, start, end],
  );
  const { selected, against } = ctx;
  const onLines = useMemo(
    () =>
      cut
        ? threadsOnLines(
            threads,
            cut.hunks.flatMap((hunk) => hunk.lines),
            selected,
            against,
          )
        : threads,
    [cut, threads, selected, against],
  );
  if (cut === undefined) return <FileLoadNote failed={whole.isError} />;
  if (cut === null)
    return (
      <div className="diff-note">
        {displayPath(file.path)} has no lines {start} to {end}
      </div>
    );
  return <WholeFile file={cut} threads={onLines} domId={domId} />;
}

function RevisionDiff({
  change,
  reference,
  block,
}: {
  change: ChangeDetail;
  reference: FileOf;
  block: number;
}) {
  const { linked, onLink } = useContext(DocumentLinkContext);
  const link = linked?.block === block ? linked.link : null;
  const { revision, lines } = reference;
  const against =
    reference.against !== null && reference.against < revision
      ? revisionVersion(reference.against)
      : baseVersion(revision);
  const { diff, files, attachKey } = useDiff(
    change,
    change.id,
    revision,
    against,
    "compare",
  );
  const drafts = useQuery(draftsQuery(change.id));
  const { shown, ported } = useThreads({
    changeNumber: change.id,
    published: change.threads,
    drafts: drafts.data?.drafts ?? NO_DRAFTS,
    selected: revision,
    against,
    includeResolved: false,
    linked: null,
    enabled: true,
  });
  const { editingTarget, setEditingTarget, setEditorDirty } = useDraftEditor();
  const navigate = useNavigate();
  const latestRevision = change.revisions.at(-1)?.number ?? revision;
  const ctx: ReviewCtx = useMemo(
    () => ({
      changeNumber: change.id,
      selected: revision,
      against,
      latestRevision,
      whitespace: "compare",
      attachKey,
      showRange: (range) => {
        void navigate(
          changeHref(change.id, range.selected, range.against, null),
        );
      },
      link,
      linkHref: (to) => changeHref(change.id, revision, against, to),
      selectLink: (to) => {
        onLink({ block, link: to });
      },
      editingTarget,
      setEditingTarget,
      setEditorDirty,
    }),
    [
      change.id,
      revision,
      against,
      latestRevision,
      attachKey,
      navigate,
      link,
      block,
      onLink,
      editingTarget,
      setEditingTarget,
      setEditorDirty,
    ],
  );
  const file = fileAt(files, reference.file);
  const threads = useMemo(
    () =>
      (file &&
        threadsByFile(shown, files, revision, against, null).get(file.path)) ??
      NO_THREADS,
    [file, shown, files, revision, against],
  );
  const busy = diff.isLoading || drafts.isLoading || ported.isLoading;
  return (
    <ReviewContext.Provider value={ctx}>
      <div className="reference-source">
        <Link to={changeHref(change.id, revision, against, null)}>
          change {change.id}
        </Link>
        , {isBase(against) ? `r${revision}` : `${against} → r${revision}`}
      </div>
      <div className="reference-diff" aria-busy={busy}>
        <DiffColumn
          layout="unified"
          ready={diff.isSuccess ? against : undefined}
        >
          {diff.isError ? (
            <div className="diff-note">The diff did not load</div>
          ) : diff.isPending ? (
            <div className="skeleton" style={{ height: 120 }} />
          ) : file === undefined ? (
            <div className="diff-note">
              {displayPath(reference.file)} is not in this diff
            </div>
          ) : lines ? (
            <FileRange
              file={file}
              {...lines}
              threads={threads}
              domId={`block-${block}`}
            />
          ) : (
            <WholeFile file={file} threads={threads} domId={`block-${block}`} />
          )}
        </DiffColumn>
      </div>
    </ReviewContext.Provider>
  );
}

/** A file of `change` that a document references: the file section of
 * the review page for it at the revision of the reference, or for a line
 * range of it. The file is a file of the diff or an attachment. `block`
 * is the first line of the reference in the text. The link of the page
 * is in `DocumentLinkContext`. */
export default function FileReference(props: {
  change: ChangeDetail;
  reference: FileOf;
  block: number;
}) {
  const { change, reference } = props;
  if (change.revisions[reference.revision] === undefined)
    return (
      <div className="diff-note">
        Change {change.id} has no revision r{reference.revision}
      </div>
    );
  return <RevisionDiff {...props} />;
}
