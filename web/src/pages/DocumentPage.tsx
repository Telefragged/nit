import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { getDocument, sendDocument } from "../api/client";
import type { Version } from "../api/document.gen";
import DiffRangeSelect from "../components/DiffRangeSelect";
import DocumentBlocks, {
  type DocumentActions,
} from "../components/document/DocumentBlocks";
import DocumentComposer from "../components/document/DocumentComposer";
import DocumentDiff from "../components/document/DocumentDiff";
import SendBar from "../components/document/SendBar";
import VersionList from "../components/document/VersionList";
import { Button } from "../components/ui/button";
import { type BlockLink, DocumentLinkContext } from "../lib/documentLink";
import {
  EMPTY,
  fromVersion,
  rangeParams,
  runRange,
  type VersionRange,
  versionLabel,
  versionRange,
} from "../lib/documentVersions";
import { referencesByLine } from "../lib/reference";
import { useLiveChanges } from "../lib/useChangeStream";
import { useDocument } from "../lib/useDocument";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { useUrlParams } from "../lib/useUrlParams";
import { ErrorPanel } from "./NotFound";

/** A document: the blocks of its text, live, with the controls that edit
 * them. Edits of authors, and of reviewers in other tabs, show as they
 * arrive. Each edit on this page goes to the server at once. Authors
 * learn of the edits when a reviewer sends the document.
 *
 * The page lists the versions of the document. With a `version`
 * parameter, the page shows the diff that the version made, without
 * controls. An `against` parameter is the version that the diff starts
 * at. */
export default function DocumentPage() {
  const number = Number(useParams().number);
  const stored = useQuery({
    queryKey: ["document", number],
    queryFn: () => getDocument(number),
  });
  const summary = stored.data;
  const live = useDocument(number);
  const { text, version, runs, error, edit, append, hold, posted, lines } =
    live;
  const [params, updateParams] = useUrlParams();
  const range = useMemo(
    () => versionRange(runs, params.get("version"), params.get("against")),
    [runs, params],
  );
  const from = range ? fromVersion(runs, range) : undefined;
  const to = range ? runs[range.to]?.version : undefined;
  const diff = useMemo(
    () => (from !== undefined && to !== undefined ? lines(from, to) : null),
    [from, to, lines],
  );
  const [linked, setLinked] = useState<BlockLink | null>(null);
  const link = useMemo(() => ({ linked, onLink: setLinked }), [linked]);
  const [editing, setEditing] = useState<{
    block: number;
    release: () => void;
  } | null>(null);
  const showRange = useCallback(
    (next: VersionRange | null) => {
      editing?.release();
      setEditing(null);
      updateParams(rangeParams(runs, next));
    },
    [editing, runs, updateParams],
  );
  const queryClient = useQueryClient();
  const send = useMutation({
    mutationFn: async (version: Version) => {
      await posted();
      return sendDocument(number, version);
    },
    onSuccess: (sent) => {
      queryClient.setQueryData(["document", number], sent);
    },
  });
  const actions: DocumentActions = useMemo(
    () => ({
      editing: editing?.block ?? null,
      onEdit: (block) => {
        editing?.release();
        setEditing(block === null ? null : { block, release: hold() });
      },
      edit,
    }),
    [editing, edit, hold],
  );
  const references = useMemo(() => referencesByLine(text ?? ""), [text]);
  const changes = useMemo(
    () =>
      [
        ...new Set(
          [...references.values()].flatMap((reference) =>
            "change" in reference ? [reference.change] : [],
          ),
        ),
      ].sort((a, b) => a - b),
    [references],
  );
  useLiveChanges(changes);
  useDocumentTitle(summary?.title ?? null);

  const failure = stored.error ?? error;
  if (failure !== null) {
    return (
      <main className="page" aria-busy={false}>
        <ErrorPanel error={failure} />
      </main>
    );
  }
  return (
    <main
      className="page document-page"
      aria-busy={summary === undefined || text === undefined}
    >
      <div className="crumb-line">
        <span>document {number}</span>
      </div>
      <div className="subject-line">
        <h1>{summary?.title}</h1>
      </div>
      {range ? (
        <div className="diffbar">
          <div className="diffbar-mode">
            <DiffRangeSelect
              items={runs}
              option={(run, index) => ({
                label: versionLabel(index),
                detail: run.writer,
              })}
              baseValue={(_, index) => String(index)}
              first={{ value: EMPTY, label: "Empty" }}
              selected={range.to}
              against={range.from === null ? EMPTY : String(range.from)}
              baseTitle="Empty = the document with no text; vM = the diff from version M"
              label="Version"
              title="The version that the diff ends at"
              placeholder="no versions"
              onLeft={(value) => {
                showRange({
                  to: range.to,
                  from: value === EMPTY ? null : Number(value),
                });
              }}
              onRight={(to) => {
                showRange(runRange(to));
              }}
            />
          </div>
          <div className="diffbar-toggles">
            <Button
              onClick={() => {
                showRange(null);
              }}
            >
              Live document
            </Button>
          </div>
        </div>
      ) : null}
      <DocumentLinkContext.Provider value={link}>
        {diff ? (
          <DocumentDiff lines={diff} />
        ) : text !== undefined ? (
          <>
            <DocumentBlocks
              text={text}
              references={references}
              actions={actions}
            />
            <DocumentComposer onAppend={append} />
          </>
        ) : (
          <div className="skeleton" style={{ height: 260 }} />
        )}
      </DocumentLinkContext.Provider>
      <VersionList
        runs={runs}
        sent={summary?.sent ?? null}
        selected={range?.to ?? null}
        onSelect={(index) => {
          showRange(runRange(index));
          window.scrollTo(0, 0);
        }}
      />
      {summary && version !== undefined && !range ? (
        <SendBar
          sent={summary.sent === version}
          sending={send.isPending}
          failure={send.error?.message ?? live.failure}
          onSend={() => {
            send.mutate(version);
          }}
        />
      ) : null}
    </main>
  );
}
