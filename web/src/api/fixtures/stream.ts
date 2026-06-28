// The mock side of WS /api/stream (docs/api.md "Events"): a per-change log the
// fixtures synthesize once, fold into a snapshot on subscribe, and append to on
// mutation — so the event-driven change page runs against the same protocol in
// mock mode (tests, screenshots) as in production. This synth log is the single
// source of truth: the REST change read folds it too (./index).

import { replayProj } from "../fold";
import type { ChangeProj, LogEntry, LogPayload, StreamMsg } from "../types";
import { changes, threads } from "./data";
import { synthLog } from "./synth";

const logs = new Map<number, LogEntry[]>();
for (const change of changes) {
  logs.set(
    change.id,
    synthLog(
      change,
      threads.filter((t) => t.change_id === change.id),
    ),
  );
}

// Appended entries (mutations, test injections) get seqs past every synthesized
// one; the fold orders per-change by idx, so the exact value only has to climb.
let nextSeq = 1_000_000;

/** A change's current synth log — the source the REST read folds (./index). */
export function logFor(changeId: number): LogEntry[] {
  return logs.get(changeId) ?? [];
}

/** A change's snapshot: its synth log folded to a ChangeProj, the same shape the
 * server ships. */
function snapshot(changeId: number): ChangeProj {
  const c = changes.find((x) => x.id === changeId);
  return replayProj({
    id: changeId,
    repo_id: c?.repo_id ?? 0,
    change_key: c?.change_key ?? "",
    entries: logFor(changeId),
  });
}

type Listener = (msg: StreamMsg) => void;
interface Sub {
  ids: Set<number>;
  listener: Listener;
}
const subs = new Set<Sub>();

export interface MockStream {
  /** Subscribe to more changes; each yields its snapshot, then its live tail. */
  add(changeIds: number[]): void;
  close(): void;
}

/** Open a mock stream. A new subscription ships the change's ChangeProj snapshot
 * (folded from its synth log), then live appends arrive as `entry` frames. */
export function mockOpenStream(listener: Listener): MockStream {
  const sub: Sub = { ids: new Set(), listener };
  subs.add(sub);
  return {
    add(changeIds) {
      for (const id of changeIds) {
        if (sub.ids.has(id)) continue;
        sub.ids.add(id);
        listener({ snapshot: snapshot(id) });
      }
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
  change_id: number,
  created_at: string,
  payload: LogPayload,
): LogEntry {
  const log = logFor(change_id);
  const entry: LogEntry = {
    change_id,
    idx: log.length,
    seq: nextSeq++,
    created_at,
    ...payload,
  };
  log.push(entry);
  logs.set(change_id, log);
  for (const sub of subs) {
    if (sub.ids.has(change_id)) sub.listener({ entry });
  }
  return entry;
}
