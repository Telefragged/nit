# Design-review guide: anti-patterns to catch

You are reviewing code for design quality. Each rule below is a real
anti-pattern that shipped in a _first draft_ in this repo and took several
review rounds to correct. The corrections were obvious in hindsight and should
not have needed pointing out one at a time. Your job as a reviewer is to catch
them here.

For each rule: what to **require**, what to **reject**, and a before/after of
what bad vs good looked like. The **meta-rule** at the end is the most
important one — read it first.

## Meta-rule: a first draft errs by _adding_, so review by _removing_

Almost every correction in the case study below was the same move: **delete a
moving part.** A column became no column. Two counters became one. A
re-derivation became a stored value. A serialized field on a struct became a
typed one. The reviewer's note was a one-liner every time ("there shouldn't be
a column", "single place", "why is this even a field?") and the fix was always
to remove, never to add.

So when you review, for each new piece the change introduces — a column, a
field, a counter, a cache, a parameter, a re-derivation, a helper that copies
data around — ask:

- Can this be **removed** entirely?
- Can it happen in **one place** instead of several?
- Can the value live in **something that already exists** instead of a new
  thing?

If the answer is yes, require that. Prefer the version with the smaller
surface even when it is a larger diff (`docs/dev.md`, golden rule 8: remove,
then change, then add). Be this voice in round one, not round eight.

## 1. Typed boundaries — no serialized blobs in domain APIs

**Require:** functions take typed domain values. Serialization (`serde_json`,
strings, byte buffers) happens only at the storage and wire edges.

**Reject:** a `serde_json::Value` (or a JSON string) threaded through an
internal/domain API. It lets the payload and its tag drift apart and pushes a
storage concern onto every caller.

```rust
// BAD — the append API speaks serialized JSON; each caller hand-builds it,
// and a (kind, payload) that disagree still compiles.
fn append_to_change(news: Vec<(LogKind, serde_json::Value)>) -> ...
let payload = serde_json::to_value(CommentPayload { comment })?;
append_to_change(conn, &entry, id, vec![(LogKind::Comment, payload)]);
```

```rust
// GOOD — the API takes a typed value and serializes internally; the kind is
// derived from the variant, so the two cannot disagree.
fn append_to_change(news: Vec<EntryPayload>) -> ...
append_to_change(conn, &entry, id, vec![EntryPayload::Comment(CommentPayload { comment })]);
```

## 2. A derived value consumers need — mint once, store in what you already have

A value derived from state (here: a comment's thread id, assigned by creation
order) that downstream consumers need (events, the log endpoint, the CLI).

**Require:** compute it **once**, at the single point where the state is
updated, and write it into the record you **already** persist and broadcast.

**Reject:** (a) re-deriving it on every read, and (b) adding a new column /
denormalized field to hold it. Both were tried here and both were wrong:

```rust
// BAD (a) — re-derive on read: the fold returns the ids, callers zip them onto
// entries, and the websocket backlog replays the whole log and slices it just
// to learn the ids for a range. Expensive and spread across every read path.
let ids = fold(...);                     // returns Vec<u64>
publish(entries.iter().zip(ids) ...)     // the "zip dance"
let all = entries_with_thread_ids(&rows)?; // full replay, then slice
```

```rust
// BAD (b) — a new column to hold the derived value, kept in sync by a startup
// backfill. More schema, more sync, and (see rule 3) it mutates the log.
"ALTER TABLE log ADD COLUMN thread_ids TEXT;"
// + a startup pass that UPDATEs old rows to match the fold
```

```rust
// GOOD — mint it once, under the lock, into the payload that is already stored
// and sent. No column, no re-derivation, no extra wire field.
fn mint_thread_id(&mut self, comment: &mut CommentInput) {
    if comment.thread_id.is_none() && !comment.body.trim().is_empty() {
        comment.thread_id = Some(self.next_thread_id);
    }
    if let Some(id) = comment.thread_id {
        self.next_thread_id = self.next_thread_id.max(id + 1);
    }
}
// the fold calls this; the id rides the existing payload.thread_id field.
```

Ask the author: _where is this value first knowable?_ Mint it there, once, into
the thing you already persist.

## 3. Append-only data stays append-only — no backfills

**Require:** new fields on an event/append-only table are optional and
forward-compatible. Old rows stay exactly as written.

**Reject:** any `UPDATE`/backfill/reconcile of an append-only log's rows. If a
new column needs values for historical rows, that is a sign the value does not
belong in a column (see rule 2). The first draft here added a `NOT NULL
DEFAULT` column **and** a startup pass that rewrote old log rows — both gone in
the final design.

```sql
-- BAD: rewriting immutable history to populate a new column.
ALTER TABLE log ADD COLUMN thread_ids TEXT NOT NULL DEFAULT '[]';
-- ... startup: UPDATE log SET thread_ids = ? WHERE seq = ?  (for old rows)
```

```text
GOOD: no column at all (rule 2). New data is additive and lives in the payload;
an entry written before the change simply does not carry it, and nothing
rewrites it.
```

## 4. One owner per invariant — no parallel state to sync

**Require:** an invariant (a counter, a cache, a "next id") is owned by exactly
one field and updated in exactly one place.

**Reject:** a second copy threaded alongside the real one "for convenience." It
will drift; here it risked a double-increment.

```rust
// BAD — minting used a local counter passed around, separate from the
// projection's own next_thread_id, so two things tracked the same number.
fn mint_thread_ids(&mut self, next_id: &mut u64) { ... *next_id += 1; }
let mut next_id = next.next_thread_id;   // a second counter
```

```rust
// GOOD — next_thread_id on the projection is the single source of truth; the
// one mint function is the only thing that touches it.
fn mint_thread_id(&mut self, comment: &mut CommentInput) { /* uses self.next_thread_id */ }
```

## 5. Compute on typed data — serialize only at the boundary

**Require:** the fold / projection / hot path works on **typed** data. JSON is
parsed once when reading a row and re-emitted only when writing a row or the
wire response.

**Reject:** a serialized `Value` stored on the in-memory type, or
(de)serialization inside the fold. A telltale: the code serializes **even on
replay**, where nothing leaves the process.

```rust
// BAD — Entry carries a serialized payload, so the fold parses it and then
// re-serializes the mutated copy back — work done on every replay too.
struct Entry { kind: LogKind, payload: serde_json::Value, ... }
fn fold(entry: &Entry) {
    let mut p: CommentPayload = entry.parse()?;   // parse in the fold
    ...
    entry.payload = serde_json::to_value(&p)?;     // re-serialize in the fold
}
```

```rust
// GOOD — Entry holds the typed payload; the fold matches it directly. JSON is
// parsed in Entry::from_row and emitted only in db::append_log / the wire view.
struct Entry { payload: EntryPayload, ... }   // typed
fn fold(change, mut entry: Entry) -> Entry { match &mut entry.payload { ... } entry }
```

## 6. Trust the project gate, not a local run

**Require:** correctness claims are backed by the project's real gate — here
`nix flake check` (a clean sandbox running `clippy -D warnings` + tests +
build). State it ran and passed.

**Reject:** "clippy is clean locally" as sufficient. A local `cargo clippy`
cache silently skipped a `clippy::pedantic` warning in the case study; the
author reported green and the warning was still there. The clean-room check
caught it.

## 7. Comments: the non-obvious _why_, or nothing

**Require:** a comment explains something the code cannot — usually _why_ (an
invariant, a non-obvious constraint, a subtle ordering).

**Reject:** comments that narrate history, restate the code, or congratulate
the author. Golden rule 10 already bans the "what it got to be" narration;
enforce it.

```rust
// BAD
// Stored entries already carry their thread ids; the returned entry is
// identical, so discard it.                      <- history + restates the code
// Commit, serializing each entry's payload to its stored JSON ...  <- restates
// Operates entirely on typed data — no JSON, so it never serializes! <- a brag
```

```rust
// GOOD
// The write lock makes this id allocation race-free against a concurrent push.
```

## Case study: "thread ids in comment events" (8 revisions)

The change: a comment's thread id is assigned by creation order in the fold, so
the stored log entry and the event it broadcasts did not carry it — consumers
had to re-fold to learn which thread a comment opened. The fix should have been
small. It took eight revisions because each draft added machinery the next
review removed.

| Round | What the draft did                                                                                | Reviewer's note (paraphrased)                   |
| ----- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| r0    | Re-derived ids on every read: fold returns ids, zip onto entries, replay+slice for the ws backlog | "extremely ugly"                                |
| r1    | Stored ids in a new `NOT NULL` log column + a startup backfill that rewrote old rows              | "so so ugly" (and it mutates the log)           |
| r2    | Made the column nullable, dropped the backfill                                                    | still a column — not needed                     |
| r3/r4 | Put the id in the existing payload, but minted via a separate pass with its own counter           | invert it; one source of truth (rule 4)         |
| r5    | Minted inside the fold, but `Entry` still held a `serde_json::Value`, so the fold re-serialized   | serialization is not a folding concern (rule 5) |
| r6    | `Entry` holds the typed payload; serialize only at the boundary                                   | (logic accepted)                                |
| r7/r8 | (no logic change) removed comments that stated what/history/brag                                  | comment the why, not the obvious (rule 7)       |

What good finally looked like: the thread id is minted **once**, in the fold,
by `ChangeProj::mint_thread_id`, which is the only writer of the single
`next_thread_id` field; it is written into the **existing** `payload.thread_id`;
the `Entry` the fold works on is **typed**; serialization happens only in
`db::append_log` and the wire view; there is **no new column, no backfill, no
second counter, no re-derivation, and no serialize-on-replay.** The net change
ended up _smaller_ than the first draft while doing strictly more.

Every one of those removals was predictable from the meta-rule. Apply it in
round one.
