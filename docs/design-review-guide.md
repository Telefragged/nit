# Design-review guide: anti-patterns to catch

You are reviewing code for design quality. Each rule below is a real
anti-pattern that shipped in a _first draft_ in this repo and took several
review rounds to correct. Your job as a reviewer is to catch them here.

For each rule: what to **require**, what to **reject**, and a before/after of
what bad vs good looked like. The **meta-rule** at the end is the most
important one — read it first.

## Meta-rule: a first draft errs by _adding_, so review by _removing_

Almost every design correction reviews here have issued was the same move:
**delete a moving part.** A column became no column. Two counters became one. A
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
surface even when it is a larger diff (`CLAUDE.md`, golden rule 8: remove,
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
fn append_to_change(news: Vec<LogPayload>) -> ...
append_to_change(conn, &entry, id, vec![LogPayload::Comment(CommentPayload { comment })]);
```

## 2. A derived value consumers need — mint once, store in what you already have

A value derived from state (here: a comment's thread id, assigned by creation
order) that downstream consumers need (events, the log endpoint, the CLI).

**Require:** compute it **once**, at the single point where the state is
updated, and write it into the record you **already** persist and broadcast.

**Reject:** (a) re-deriving it on every read, and (b) adding a new column /
denormalized field to hold it:

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
belong in a column (see rule 2).

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
will drift — two owners of one counter double-increment it.

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
// BAD — LogEntry carries a serialized payload, so the fold parses it and then
// re-serializes the mutated copy back — work done on every replay too.
struct LogEntry { kind: LogKind, payload: serde_json::Value, ... }
fn fold(entry: &LogEntry) {
    let mut p: CommentPayload = entry.parse()?;   // parse in the fold
    ...
    entry.payload = serde_json::to_value(&p)?;     // re-serialize in the fold
}
```

```rust
// GOOD — LogEntry holds the typed payload; the fold matches it directly. JSON
// is parsed once where rows are read and emitted only where rows are written.
struct LogEntry { payload: LogPayload, ... }   // typed
fn fold(change, mut entry: LogEntry) -> LogEntry { match &mut entry.payload { ... } entry }
```

## 6. Let the types make illegal states unrepresentable

**Require:** the type system carries the invariant, not a runtime check or
a convention:

- **A closed set of values is an `enum`, never a `String`** (sides,
  verdicts, statuses, kinds…) — the rule and its payoff are in
  `crates/nit-types/src/domain.rs`, from which the TS unions in
  `web/src/api/types.gen.ts` are generated. A `String` is fine only at the
  storage boundary, converted to the enum immediately.
- **Absence is not a state — model it.** Encode the legal combinations of
  a cluster of `Option`s as an enum so the illegal ones can't be built: a
  thread's location is `Anchor` (`Change | File | Line { … }`,
  `crates/nit-types/src/fold.rs`), not five loose `Option`s.
- **One input names one thing.** Identify a thing two ways with two
  type-distinct flags, not one that sniffs the value's form: `nit comment`
  takes `--change <u64>` or `--change-id <String>`, never one flag that
  guesses.

**Reject:** a stringly-typed field where an enum fits, a cluster of
`Option`s whose combinations encode the real states, an input that guesses
what it names. A violation is a finding to fix, not a style preference.
