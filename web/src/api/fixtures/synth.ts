// Synthesize a change's append-only event log from its mock records — the
// fixtures' single source of truth for a change's review state. The websocket
// projects it and the REST change read folds it (./index), both through the
// shared crates/nit-wasm fold, so the two never disagree. Thread ids are
// emitted explicitly so the fold reproduces the record ids the reviewer's
// drafts reference; a review's id is its entry's `position`, so the records number
// their reviews by where this places them.

import type { CommentInput, LogEntry, LogPayload } from "../types";
import type { ChangeRecord, ThreadRecord } from "./store";

// Monotonic across all changes — the global `sequence` ordering of the real log.
let sequence = 0;

/** One thread comment → its wire `CommentInput`. The opening comment (index 0)
 * carries the anchor + explicit thread id; the last carries the rolled-up
 * `resolved` state, so the fold lands on the record's final value. */
function commentInput(
  thread: ThreadRecord,
  body: string,
  open: boolean,
  last: boolean,
): CommentInput {
  return {
    thread_id: thread.id,
    revision: open ? thread.revision : null,
    anchor: open ? thread.anchor : null,
    body,
    resolved: last ? thread.resolved : null,
  };
}

/** A change's records → its log, ascending by `position`: every revision, then the
 * reviews and author comments that opened and answered its threads, in time
 * order, then a terminal lifecycle entry. */
export function synthLog(
  change: ChangeRecord,
  threads: ThreadRecord[],
): LogEntry[] {
  const entries: LogEntry[] = [];
  const add = (created_at: string, payload: LogPayload) =>
    entries.push({
      change_number: change.id,
      position: entries.length,
      sequence: sequence++,
      created_at,
      ...payload,
    });

  for (const r of change.revisions) {
    add(r.created_at, {
      kind: "revision",
      payload: {
        commit_sha: r.commit_sha,
        parent_sha: r.parent_sha,
        fork_sha: r.fork_sha,
        message: r.message,
        // No pure-rebase fixtures: every revision resets status, matching the
        // mock's own per-revision status derivation.
        resets_status: true,
      },
    });
  }

  // Group each thread comment under the review that published it (one review
  // entry carries all of its comments) or emit it as a standalone author
  // comment. `sort` breaks created_at ties so a review opens a thread before
  // an author reply in the same instant.
  const events: { created_at: string; sort: number; payload: LogPayload }[] =
    [];
  const byReview = new Map<number, CommentInput[]>();
  for (const t of threads) {
    t.comments.forEach((c, i) => {
      const input = commentInput(
        t,
        c.body,
        i === 0,
        i === t.comments.length - 1,
      );
      if (c.review_id === null) {
        events.push({
          created_at: c.created_at,
          sort: 1,
          payload: { kind: "comment", payload: input },
        });
      } else {
        const list = byReview.get(c.review_id) ?? [];
        list.push(input);
        byReview.set(c.review_id, list);
      }
    });
  }
  for (const r of change.reviews) {
    events.push({
      created_at: r.created_at,
      sort: 0,
      payload: {
        kind: "review",
        payload: {
          revision: r.revision,
          verdict: r.verdict,
          message: r.message,
          comments: byReview.get(r.id) ?? [],
        },
      },
    });
  }
  events.sort((a, b) =>
    a.created_at < b.created_at
      ? -1
      : a.created_at > b.created_at
        ? 1
        : a.sort - b.sort,
  );
  for (const ev of events) add(ev.created_at, ev.payload);

  if (change.terminal) {
    const tip = change.revisions[change.revisions.length - 1];
    add(tip?.created_at ?? "", {
      kind: "lifecycle",
      payload:
        change.terminal === "merged"
          ? {
              action: "merged",
              commit_sha: tip?.commit_sha ?? null,
              message: null,
            }
          : { action: "abandoned", message: null },
    });
  }

  return entries;
}
