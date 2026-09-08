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
  /** The changes whose projection the subscriber has. */
  announced: Set<number>;
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

/** Sends the change's projection to a subscriber that has not got it. */
function announce(sub: Sub, changeNumber: number) {
  if (sub.announced.has(changeNumber)) return;
  sub.announced.add(changeNumber);
  sub.listener({ projection: projection(changeNumber) });
}

export interface MockStream {
  /** Follow the changes the subscription picks; replaces the previous one. */
  subscribe(subscription: Subscription): void;
  close(): void;
}

/** Open a mock stream. A subscription ships the projection of every change
 * its query picks (folded from the synth log), then live appends arrive as
 * `entry` frames, behind the projection of a change the subscriber meets
 * for the first time. `after` is ignored: there is no backlog the
 * projections do not hold. */
export function mockOpenStream(listener: Listener): MockStream {
  const sub: Sub = { query: null, announced: new Set(), listener };
  subs.add(sub);
  return {
    subscribe(subscription) {
      sub.query = subscription.query;
      sub.announced.clear();
      for (const c of changes)
        if (picks(subscription.query, c)) announce(sub, c.id);
    },
    close() {
      subs.delete(sub);
    },
  };
}

/** Append one entry to a change's synth log and push it as a live `entry` frame
 * to its subscribers — the fixtures' analog of the server's append broadcast.
 * Drives the mock's own mutations (submit/abandon) and test event injection. */
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
  const c = changes.find((x) => x.id === change_number);
  for (const sub of subs) {
    if (sub.query === null || c === undefined || !picks(sub.query, c)) continue;
    announce(sub, change_number);
    sub.listener({ entry });
  }
  return entry;
}
