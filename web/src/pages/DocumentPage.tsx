import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { getDocument, sendDocument } from "../api/client";
import type { Version } from "../api/document.gen";
import DocumentBlocks, {
  type DocumentActions,
} from "../components/document/DocumentBlocks";
import DocumentComposer from "../components/document/DocumentComposer";
import SendBar from "../components/document/SendBar";
import { type BlockLink, DocumentLinkContext } from "../lib/documentLink";
import { referencesByLine } from "../lib/reference";
import { useLiveChanges } from "../lib/useChangeStream";
import { useDocument } from "../lib/useDocument";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { ErrorPanel } from "./NotFound";

/** A document: the blocks of its text, live, with the controls that edit
 * them. An edit of the author, or of the reviewer in another tab, shows
 * as it arrives. Each edit of the reviewer goes to the server at once.
 * The author learns of the edits when the reviewer sends the document. */
export default function DocumentPage() {
  const number = Number(useParams().number);
  const stored = useQuery({
    queryKey: ["document", number],
    queryFn: () => getDocument(number),
  });
  const summary = stored.data;
  const live = useDocument(number);
  const { text, version, error, edit, append, hold, posted } = live;
  const [linked, setLinked] = useState<BlockLink | null>(null);
  const link = useMemo(() => ({ linked, onLink: setLinked }), [linked]);
  const [editing, setEditing] = useState<{
    block: number;
    release: () => void;
  } | null>(null);
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
      <DocumentLinkContext.Provider value={link}>
        {text !== undefined ? (
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
        />
      ) : null}
    </main>
  );
}
