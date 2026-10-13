// The mock side of WS /api/stream: the single source of
// truth for mock mode. The REST change read (./index) folds this same synth
// log, so mock WS and mock REST agree in tests/screenshots as they do in prod.

import { changeDetail, replayProjection } from "../fold";
import type {
  ChangeProjection,
  ChangeQuery,
  LogEntry,
  LogPayload,
  StreamMessage,
  Subscription,
} from "../types";
import { changes, threads } from "./data";
import type { ChangeRecord } from "./store";
import { synthLog } from "./synth";

const logs = new Map<number, LogEntry[]>();
for (const change of changes) {
  logs.set(
    change.id,
    synthLog(
      change,
      threads.filter((t) => t.change_number === change.id),
    ),
  );
}

// Appended entries (mutations, test injections) get seqs past every synthesized
// one; the fold orders per-change by position, so the exact value only has to climb.
let nextSeq = 1_000_000;

/** A change's current synth log — the source the REST read folds (./index). */
export function logFor(changeNumber: number): LogEntry[] {
  return logs.get(changeNumber) ?? [];
}

/** A change's projection: its synth log folded to a ChangeProjection, the same shape the
 * server ships. */
export function projection(changeNumber: number): ChangeProjection {
  const c = changes.find((x) => x.id === changeNumber);
  return replayProjection({
    id: changeNumber,
    repo_id: c?.repo_id ?? 0,
    change_id: c?.change_id ?? "",
    entries: logFor(changeNumber),
  });
}

type Listener = (msg: StreamMessage) => void;
interface Sub {
  query: ChangeQuery | null;
  /** The position of the next entry to send of each followed change. */
  positions: Map<number, number>;
  listener: Listener;
}
const subs = new Set<Sub>();

/** Whether `query` picks the change now: the rule the server applies, the
 * status the folded latest revision's. Shared with the mock change list, so
 * mock REST and mock WS pick the same set. */
export function picks(query: ChangeQuery, c: ChangeRecord): boolean {
  return (
    (query.repo === undefined || query.repo === c.repo_id) &&
    (query.change === undefined || query.change === c.id) &&
    (query.change_id === undefined || query.change_id === c.change_id) &&
    (query.tag ?? []).every((t) => {
      const at = t.indexOf("=");
      return c.tags?.[t.slice(0, at)] === t.slice(at + 1);
    }) &&
    (query.status === undefined ||
      query.status.length === 0 ||
      query.status.includes(
        changeDetail(projection(c.id)).revisions.at(-1)?.status ?? "pending",
      ))
  );
}

/** Sends the messages that the subscriber lacks, in the order of the server
 * read: the matched changes first, then `left`. The mock starts each new
 * change at its projection, whatever `start` says. */
function sync(sub: Sub) {
  const query = sub.query;
  if (query === null) return;
  const left: number[] = [];
  for (const c of changes) {
    const held = sub.positions.get(c.id);
    if (!picks(query, c)) {
      if (held !== undefined) left.push(c.id);
      continue;
    }
    const log = logFor(c.id);
    sub.positions.set(c.id, log.length);
    if (held === undefined) {
      sub.listener({ projection: projection(c.id) });
      continue;
    }
    for (const entry of log.slice(held)) sub.listener({ entry });
  }
  for (const id of left) {
    sub.positions.delete(id);
    sub.listener({ left: id });
  }
}

export interface MockStream {
  /** Follow the changes the subscription picks; replaces the previous one. */
  subscribe(subscription: Subscription): void;
  close(): void;
}

/** Open a mock stream. A subscription ships the projection of every change
 * its query matches (folded from the synth log), and the entries from the
 * positions of the changes that it holds on. Then each append sends what the
 * server sends after a commit. */
export function mockOpenStream(listener: Listener): MockStream {
  const sub: Sub = { query: null, positions: new Map(), listener };
  subs.add(sub);
  return {
    subscribe(subscription) {
      sub.query = subscription.query;
      sub.positions = new Map(
        Object.entries(subscription.positions ?? {}).map(([id, position]) => [
          Number(id),
          position,
        ]),
      );
      sync(sub);
    },
    close() {
      subs.delete(sub);
    },
  };
}

/** Append one entry to a change's synth log and send each subscriber the
 * messages that it lacks. This is the fixtures' analog of a server commit,
 * and it drives the mock's own mutations (submit/abandon) and test event
 * injection. */
export function mockAppend(
  change_number: number,
  created_at: string,
  payload: LogPayload,
): LogEntry {
  const log = logFor(change_number);
  const entry: LogEntry = {
    change_number,
    position: log.length,
    sequence: nextSeq++,
    created_at,
    ...payload,
  };
  log.push(entry);
  logs.set(change_number, log);
  for (const sub of subs) sync(sub);
  return entry;
}
