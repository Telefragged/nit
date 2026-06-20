## Events

A change owns a log; a chain is a path over a set of changes. A follower
watches a **set** of changes over **one websocket**, choosing its own
membership; the server tracks only the subscription set — no per-follower
chain, no resubscribe bookkeeping.

- `WS /api/stream?repo={id}` — the client-driven change stream. The client
  builds its subscription over the open socket; the server emits **only**
  entries for currently-subscribed changes.

  ```jsonc
  // client → server
  {"subscribe": {"10": 4, "11": 0}}   // change_id → from-idx: replay [from, head) then stream live
  // server → client
  {"change_id": 10, "idx": 5, "seq": 412, "kind": "review", "created_at": "…", "payload": {…}}
  {"new_parent": {"of": 10, "parent": 9}}    // out-of-log: change 10's parent edge is now change 9
  ```

  A `subscribe` arms the change's live feed **before** replaying its
  `[from, head)` backlog, then drops live entries with
  `idx < last_backlog_idx + 1` — the arm/read overlap is a duplicate the
  watermark suppresses, never a gap. The server joins the subscribed changes'
  per-change feeds in a keyed dynamic-membership map (`tokio-stream`'s
  `StreamMap`); there is no per-chain channel and no server-side chain —
  following a whole chain is the client subscribing to each member, and a
  follower drops the whole set by closing the socket. A follower that falls
  more than a feed's buffer behind **overflows**: the server closes the socket
  rather than skip the gap, and the client reconnects and re-reads the missed
  entries from the log.

  The **only** non-log message is `new_parent` (out-of-log, no `idx`/`seq`):
  it fires whenever a parent↔child edge `{of → parent}` is newly established —
  an existing change re-roots onto a new parent, **or** a brand-new child is
  stacked on an existing parent (a chain extension) — and the client re-derives
  its logical chain and subscribes the new member. It is published on the
  edge's **pre-existing** endpoint, the only feed a follower can already hold:
  the re-rooted change's own feed for a re-root, the parent's feed for a new
  child (whose own feed has no subscribers yet). It is **advisory and
  idempotent** — the next HEAD re-derivation supersedes it, so a dropped one
  costs nothing (a follower re-derives from local HEAD each pass anyway).

```jsonc
TaggedLogEntry = {"change_id": 10, "idx": 5, "seq": 412, "kind": "…", "created_at": "…", "payload": {…}}
ClientMsg      = {"subscribe": {"<id>": <from-idx>, …}}
NewParent      = {"new_parent": {"of": 10, "parent": 9}}
```

### The cursor

The follower's resume state is a **vector** `change_id → idx` (the count of
that change's entries consumed). An **absent key ⇒ 0**, so a change newly
stacked into a chain replays from the start; a change that left the path keeps
its (inert) key. `subscribe` is the vector handed to the server, expanded to
explicit zeros. A `nit wait`/`nit log --follow` return prints the advanced
vector; the agent passes it back next call. The wake rule (which entries end a
parked `nit wait`) is a **client** concern (docs/data-model.md "Wake rule"):
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

| state                | when                                                                                                     | actionable |
| -------------------- | -------------------------------------------------------------------------------------------------------- | ---------- |
| `merged`             | every non-abandoned member merged at its pinned revision (off the main page)                             | true       |
| `agents_turn`        | else any member changes_requested/commented; or empty/all-abandoned tip; or all approved while `partial` | true       |
| `waiting_for_review` | else any member pending                                                                                  | false      |
| `approved`           | else all approved (≥1) and not `partial`                                                                 | true       |

`actionable` ≡ `state != waiting_for_review`. A chain drops off the main page
iff **every** member is terminal — any one live member keeps a partially-landed
stack visible.
