## Events

A change owns a log; a chain is a path over a set of changes. A follower
watches a **set** of changes over **one websocket**, choosing its own
membership; the server tracks only the subscription set — no per-follower
chain, no resubscribe bookkeeping.

- `WS /api/stream?repo={id}` — the client-driven change stream. The client
  builds its subscription over the open socket; the server emits **only**
  frames for currently-subscribed changes. Server → client frames are a tagged
  `StreamMsg`: a `snapshot` (a change's folded `ChangeProj`) or an `entry` (one
  log entry). The client picks one of two subscribe modes per message:

  ```jsonc
  // client → server — cursor replay (the CLI follower)
  {"subscribe": {"10": 4, "11": 0}}    // change_id → from-idx: replay [from, head) then stream live
  // client → server — snapshot mode (the web change page)
  {"subscribe_snapshot": ["10", "11"]} // per change: a ChangeProj snapshot, then the live tail

  // server → client
  {"entry": {"change_id": 10, "idx": 5, "seq": 412, "created_at": "…", "kind": "review", "payload": {…}}}
  {"snapshot": {"id": 10, "repo_id": 1, "revisions": […], "threads": […], "reviews": […], "entries_folded": 5, …}}
  ```

  Either mode arms the change's live feed **before** reading its backlog — a
  `[from, head)` entry replay (cursor) or a snapshot of the in-memory
  projection — so an append that lands mid-read rides the feed and is deduped
  by an idx watermark, never gapped. **Snapshot mode** ships the change's
  already-folded `ChangeProj` — the fold the server has done once, not repeated
  per follower — whose `entries_folded` is the high-water mark; the server drops
  live entries below it, so a follower resumes folding the live tail at the
  boundary with no overlap, and a reconnect re-snapshots rather than tracking a
  cursor. **Cursor mode** replays raw `[from, head)` entries and drops live ones
  with `idx < last_backlog_idx + 1`. The server joins the subscribed changes'
  per-change feeds in a keyed dynamic-membership map (`tokio-stream`'s
  `StreamMap`); there is no per-chain channel and no server-side chain —
  following a whole chain is the client subscribing to each member, and a
  follower drops the whole set by closing the socket. A follower that falls
  more than a feed's buffer behind **overflows**: the server closes the socket
  rather than skip the gap, and the client reconnects and re-reads (or
  re-snapshots) the missed state. A chain member newly stacked while a follower
  is parked lands on its own feed, so a follower learns of it by re-deriving
  from local HEAD and resubscribing, not from any server push.

```jsonc
StreamMsg = {"entry": TaggedLogEntry} | {"snapshot": ChangeProj}
ClientMsg = {"subscribe": {"<id>": <from-idx>, …}} | {"subscribe_snapshot": ["<id>", …]}
TaggedLogEntry = {"change_id": 10, "idx": 5, "seq": 412, "created_at": "…", "kind": "…", "payload": {…}}
```

### The cursor

The follower's resume state is a **vector** `change_id → idx` (the count of
that change's entries consumed). An **absent key ⇒ 0**, so a change newly
stacked into a chain replays from the start; a change that left the path keeps
its (inert) key. `subscribe` is the vector handed to the server, expanded to
explicit zeros. A `nit log --wait`/`--follow` return prints the advanced
vector; the agent passes it back next call. The wake rule (which entries end a
parked `nit log --wait`) is a **client** concern (docs/data-model.md "Wake rule"):
the server ships raw tagged entries.

### State table (normative)

A change's **displayed status** is per `(change, revision)`: the verdict of
the latest review whose `revision` equals the patchset a path pins, falling
back to `pending`. `merged`/`abandoned` are terminal.

```
status:  pending | approved | changes_requested | commented | merged | abandoned
```

A chain's **derived state** is a pure read-time function of its members, each
at the revision the tip pins. **Abandonment is derivation-inert**: an
`abandoned` member is excluded from the rollup entirely (no chain-level
abandoned state exists) — it shows as `abandoned` on its own path entry, and
the agent decides what to do with it.

| state                | when                                                                         | actionable |
| -------------------- | ---------------------------------------------------------------------------- | ---------- |
| `merged`             | every non-abandoned member merged at its pinned revision (off the main page) | true       |
| `agents_turn`        | else any member changes_requested/commented; or empty/all-abandoned tip      | true       |
| `waiting_for_review` | else any member pending                                                      | false      |
| `approved`           | else all approved (≥1)                                                       | true       |
