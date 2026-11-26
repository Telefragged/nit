import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { getDocument, sendDocument } from "../api/client";
import type { Version } from "../api/document.gen";
import DocumentBlocks, {
  type DocumentActions,
} from "../components/document/DocumentBlocks";
import DocumentComposer from "../components/document/DocumentComposer";
import DocumentDiff from "../components/document/DocumentDiff";
import DocumentHistory from "../components/document/DocumentHistory";
import SendBar from "../components/document/SendBar";
import VersionList from "../components/document/VersionList";
import { type BlockLink, DocumentLinkContext } from "../lib/documentLink";
import {
  extendedRange,
  oneVersion,
  rangeLabel,
  rangeParams,
  type VersionRange,
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
 * A button of the bar opens the history of the document: its versions,
 * in a popover. A press on a version shows the text at that version,
 * without controls, and a press with Shift shows the diff from the
 * version on screen. The `version` and `against` parameters of the
 * address contain the range. The Live button of the popover shows the
 * live document again. */
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
  const from = range ? runs[range.from]?.version : undefined;
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
      <div className="subject-line">
        <h1>{summary?.title}</h1>
      </div>
      <DocumentLinkContext.Provider value={link}>
        {diff ? (
          <DocumentDiff
            lines={diff}
            only={range?.from === range?.to ? "new" : undefined}
          />
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
      {summary && version !== undefined ? (
        <SendBar
          sent={summary.sent === version}
          sending={send.isPending}
          failure={send.error?.message ?? live.failure}
          onSend={() => {
            send.mutate(version);
          }}
          history={
            <DocumentHistory
              shown={range && rangeLabel(range)}
              onLive={() => {
                showRange(null);
              }}
            >
              <VersionList
                runs={runs}
                sent={summary.sent}
                selected={range}
                onSelect={(index, extend) => {
                  showRange(
                    extend ? extendedRange(range, index) : oneVersion(index),
                  );
                }}
              />
            </DocumentHistory>
          }
          live={range === null}
        />
      ) : null}
    </main>
  );
}
