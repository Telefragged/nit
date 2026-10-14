import { useEffect, useMemo, useRef } from "react";

import { changeDetail, foldEntry, replayProjection } from "../api/fold";
import { openStream, type StreamHandle } from "../api/stream";
import type {
  ChangeProjection,
  ChangeStatus,
  LogEntry,
  Tags,
} from "../api/types";
import { verdictStatus } from "../api/verdict";

/** What a `tags` entry does to the tags the change held before it: each
 * pair it adds, and each pair whose value it replaces. */
interface TagChange {
  added: string[];
  removed: string[];
}

/** An entry, with what the fold of the entries before it says about it. */
export interface Note {
  entry: LogEntry;
  /** The number of the revision that a `revision` entry creates. */
  revision: number;
  /** Whether a `revision` entry reset a review status to pending. */
  reset: boolean;
  tags: TagChange;
}

const verdictStatuses = new Set<ChangeStatus>(Object.values(verdictStatus));

function tagChange(held: Tags, tags: Tags): TagChange {
  const change: TagChange = { added: [], removed: [] };
  for (const [key, value] of Object.entries(tags)) {
    const old = held[key];
    if (old === value) continue;
    change.added.push(`${key}=${value}`);
    if (old !== undefined) change.removed.push(`${key}=${old}`);
  }
  return change;
}

/** Notes `entry` against `before`, the fold of the entries before it. */
function noteFor(before: ChangeProjection, entry: LogEntry): Note {
  const note: Note = {
    entry,
    revision: 0,
    reset: false,
    tags: { added: [], removed: [] },
  };
  if (entry.kind === "revision") {
    const revisions = changeDetail(before).revisions;
    const status = revisions.at(-1)?.status;
    note.revision = revisions.length;
    note.reset =
      entry.payload.resets_status &&
      status !== undefined &&
      verdictStatuses.has(status);
  }
  if (entry.kind === "tags") {
    note.tags = tagChange(changeDetail(before).tags ?? {}, entry.payload.tags);
  }
  return note;
}

/** The notes of a change's log, oldest first, as a store that
 * `useSyncExternalStore` reads. */
export class ChangeLogFeed {
  private entries: LogEntry[] = [];
  private proj: ChangeProjection | null = null;
  private current: Note[] = [];
  private listeners = new Set<() => void>();

  constructor(readonly changeNumber: number) {}

  /** The count of entries that the feed holds: where a resubscription
   * resumes. */
  get held() {
    return this.entries.length;
  }

  /** Takes the next entry of the log. An entry of another change, or one
   * that the feed holds, arrives from a subscription that the socket
   * replaced, and the feed drops it. */
  push(entry: LogEntry) {
    if (
      entry.change_number !== this.changeNumber ||
      entry.position !== this.entries.length
    )
      return;
    this.entries.push(entry);
    this.advance();
  }

  /** Starts the fold. The fold needs the identity of the change to build a
   * projection, so entries wait until the identity is known. */
  identify(repo_id: number, change_id: string) {
    if (this.proj !== null) return;
    this.proj = replayProjection({
      id: this.changeNumber,
      repo_id,
      change_id,
      entries: [],
    });
    this.advance();
  }

  /** Folds each entry that has no note yet, once. */
  private advance() {
    let proj = this.proj;
    if (proj === null || this.current.length === this.entries.length) return;
    const notes = [...this.current];
    for (const entry of this.entries.slice(notes.length)) {
      notes.push(noteFor(proj, entry));
      proj = foldEntry(proj, entry);
    }
    this.proj = proj;
    this.current = notes;
    for (const listener of this.listeners) listener();
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  notes = () => this.current;
}

/** The log of a change, followed from mount. `identity` arrives with the
 * projection, and the notes start when it does.
 *
 * One socket serves every change that the page shows. Navigation sends it
 * a new subscription, because a browser can delay a new socket to a host
 * until the previous one closes. */
export function useChangeLog(
  changeNumber: number,
  identity: { repo_id: number; change_id: string } | undefined,
): ChangeLogFeed {
  const feed = useMemo(() => new ChangeLogFeed(changeNumber), [changeNumber]);
  const shown = useRef(feed);
  const stream = useRef<StreamHandle | null>(null);
  useEffect(() => {
    const handle = openStream(
      (msg) => {
        if ("entry" in msg) shown.current.push(msg.entry);
      },
      () => ({ [shown.current.changeNumber]: shown.current.held }),
    );
    stream.current = handle;
    return () => {
      handle.close();
    };
  }, []);
  useEffect(() => {
    shown.current = feed;
    stream.current?.subscribe({
      query: { change: feed.changeNumber },
      start: "log",
    });
  }, [feed]);
  const repoId = identity?.repo_id;
  const changeId = identity?.change_id;
  useEffect(() => {
    if (repoId !== undefined && changeId !== undefined)
      feed.identify(repoId, changeId);
  }, [feed, repoId, changeId]);
  return feed;
}
