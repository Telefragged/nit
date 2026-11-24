import { useCallback, useEffect, useRef, useState } from "react";
import { getDocumentSnapshot, postDocumentUpdate } from "../api/client";
import type { Run, Version } from "../api/document.gen";
import type { Line } from "../api/types";
import { followDocument } from "../api/documentStream";
import { errorMessage } from "./utils";
import { DocumentReplica } from "./wasm";

interface Live {
  replica: DocumentReplica;
  synced: Version;
  posts: Promise<void>;
  held: Uint8Array[] | null;
}

/** The reviewer's replica of a document, with what it shows. */
export interface LiveDocument {
  /** The Markdown text of the replica, absent until the snapshot loads. */
  text: string | undefined;
  version: Version | undefined;
  /** The versions of the document, the oldest first. */
  runs: readonly Run[];
  /** The failure of the load, or null. */
  error: unknown;
  /** Why the last edit did not reach the server, or null. */
  failure: string | null;
  /** Changes the text of the replica to `change` of it, as one splice,
   * and posts the edit. */
  edit: (change: (text: string) => string) => void;
  /** Adds `markdown` at the end of the text as a block of its own, and
   * posts the edit. */
  append: (markdown: string) => void;
  /** Stops the merge of incoming updates, so the text of the replica
   * stays the text that an open editor started from. Returns the function
   * that merges the updates that arrived since. */
  hold: () => () => void;
  /** Resolves after the post of every edit so far. */
  posted: () => Promise<void>;
  /** Every line of the texts at two versions of the replica, as the
   * lines of a diff from `from` to `to`. */
  lines: (from: Version, to: Version) => Line[];
}

const NO_RUNS: readonly Run[] = [];

/** The reviewer's replica of document `number`, live.
 *
 * The hook loads the snapshot into a replica and merges each update of
 * the document stream. The WebAssembly module must be ready (FoldReady). */
export function useDocument(number: number): LiveDocument {
  const live = useRef<Live | null>(null);
  const [shown, setShown] = useState<{
    number: number;
    text: string;
    version: Version;
    runs: Run[];
  }>();
  const [failed, setFailed] = useState<{ number: number; error: unknown }>();
  const [failure, setFailure] = useState<string | null>(null);

  const show = useCallback(
    (replica: DocumentReplica) => {
      setShown({
        number,
        text: replica.text(),
        version: replica.version(),
        runs: replica.runs() as Run[],
      });
    },
    [number],
  );
  const merge = useCallback(
    (replica: DocumentReplica, updates: readonly Uint8Array[]) => {
      const before = replica.version();
      for (const update of updates) replica.merge(update);
      if (replica.version() !== before) show(replica);
    },
    [show],
  );

  useEffect(() => {
    let stop: (() => void) | undefined;
    let closed = false;
    getDocumentSnapshot(number)
      .then((snapshot) => {
        if (closed) return;
        const replica = new DocumentReplica(snapshot);
        const doc: Live = {
          replica,
          synced: replica.version(),
          posts: Promise.resolve(),
          held: null,
        };
        live.current = doc;
        show(replica);
        stop = followDocument(
          number,
          () => replica.version(),
          (update) => {
            if (doc.held) doc.held.push(update);
            else merge(replica, [update]);
          },
        );
      })
      .catch((error: unknown) => {
        if (!closed) setFailed({ number, error });
      });
    return () => {
      closed = true;
      stop?.();
      live.current?.replica.free();
      live.current = null;
    };
  }, [number, show, merge]);

  const write = useCallback(
    (edit: (replica: DocumentReplica) => void) => {
      const doc = live.current;
      if (!doc) return;
      const before = doc.replica.version();
      edit(doc.replica);
      if (doc.replica.version() === before) return;
      show(doc.replica);
      const version = doc.replica.version();
      const update = doc.replica.updates_since(doc.synced);
      doc.posts = doc.posts
        .then(() => postDocumentUpdate(number, update))
        .then(
          () => {
            doc.synced = version;
            setFailure(null);
          },
          (error: unknown) => {
            setFailure(errorMessage(error));
          },
        );
    },
    [number, show],
  );

  const edit = useCallback(
    (change: (text: string) => string) => {
      write((replica) => {
        replica.set_text(change(replica.text()));
      });
    },
    [write],
  );
  const append = useCallback(
    (markdown: string) => {
      write((replica) => {
        replica.append(markdown);
      });
    },
    [write],
  );

  const hold = useCallback(() => {
    const doc = live.current;
    if (!doc) return () => undefined;
    const held: Uint8Array[] = [];
    doc.held = held;
    return () => {
      if (doc.held !== held) return;
      doc.held = null;
      if (live.current === doc) merge(doc.replica, held);
    };
  }, [merge]);

  const posted = useCallback(
    () => live.current?.posts ?? Promise.resolve(),
    [],
  );

  const lines = useCallback(
    (from: Version, to: Version) =>
      (live.current?.replica.lines_between(from, to) ?? []) as Line[],
    [],
  );

  const current = shown?.number === number ? shown : undefined;
  return {
    text: current?.text,
    version: current?.version,
    runs: current?.runs ?? NO_RUNS,
    error: failed?.number === number ? failed.error : null,
    failure,
    edit,
    append,
    hold,
    posted,
    lines,
  };
}
