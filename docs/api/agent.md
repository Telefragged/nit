## Agent endpoints

The agent drives the loop with a per-change cursor it owns (a vector of
`change_id → idx`); `nit push`/`nit comment` return no index, so an entry
that lands between two of its own actions is never skipped
(docs/agent-workflow.md). One-shot reads (`nit status`, `nit log`) read the
cursor's gap; the live followers (`nit wait`, `nit log --follow`) drive it
over the websocket ("Events").

- `POST /api/changes/{id}/comments` —
  `req: {"thread_id": null, "revision": 2, "file": "Cargo.toml", "line": 14, "side": "new", "range": CommentRange, "body": "…", "resolved": false}`
  → Thread (the comment's `review_id` is null → agent, published
  immediately). The agent's single
  comment-posting path — the change is the request **target**, like the draft
  and review endpoints (so attribution never depends on the server guessing
  "where the agent is"). With no `thread_id` it opens a **new thread** on the
  change, anchored exactly like a reviewer draft (`file`/`line`/`side`/`range`,
  same rules); `revision` is optional and defaults to the change's latest (the
  just-pushed one), but may name any earlier revision to pin the thread to a
  prior patchset. With a **`thread_id`** it appends a reply to that thread on
  this change (anchor fields ignored — the thread owns the anchor). `body` is
  required (non-empty), except a `thread_id` reply may carry an empty body when
  it only changes `resolved`. `resolved` is the thread-resolution decision: on a
  new thread, `false`/omitted leaves it **open** and `true` opens it **already
  resolved**; on a reply, `true` resolves / `false` reopens / omitted leaves it
  unchanged. An agent comment never changes the change's review status (it is
  not a verdict). Appends one `comment` log entry; returns no cursor. Used by
  `nit comment`. (Why an agent comments at all: docs/agent-workflow.md
  "Annotate the choices you make".)
- `POST /api/changes/{id}/abandon` → ChangeDetail — mark a change
  **abandoned** (`nit abandon`): a reviewer/agent judgment that this change is
  dead, never an automatic decision. Optional `req: {"message": "…"}` records a
  reason. Appends a `lifecycle{abandoned}` entry; a no-op on an already-terminal
  change. Abandonment is a **per-change status only** — it does not change any
  chain's derived `state` or membership (the change stays a member, and a tip if
  it is a leaf); the agent reads the per-change `abandoned` and decides whether
  to drop the change or pause (docs/data-model.md "Lifecycle"). Durable:
  reversible only by `reopen`.
- `POST /api/changes/{id}/reopen` → ChangeDetail — clear an `abandoned`
  change back to its retained verdict status (`nit reopen`), so the agent may
  push a new revision (which folds it to `pending`). Appends a
  `lifecycle{reopened}` entry. A no-op on a non-abandoned change.

```json
LogEntry = {
  "change_id": 10,          // which change's log this entry belongs to
  "idx": 5,                 // 0-based position in THAT change's log
  "seq": 412,               // global, monotone across the repo (cross-change order)
  "created_at": "…",
  "kind": "review",         // revision | review | comment | lifecycle
  "payload": { … }          // shape determined by kind; data-model.md "Payloads"
}
```

The API ships only the raw entry. The one-line digest behind `--oneline` is
**not** an API field: it is a client display concern, derived from `kind` +
`payload` on demand (in the CLI). The aggregated chain log
(`GET /api/chains/{change_id}/log`) returns these entries merged across the
chain's members and sorted by `seq`.
