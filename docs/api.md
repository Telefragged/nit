# HTTP API — the contract

Everything under `/api`, JSON in/out. **This file is the single source of
truth for shapes**: the frontend mirrors it in `web/src/api/types.ts`, the
backend in `crates/nit/src/api/types.rs`. Change shapes here first.

Errors: non-2xx with `{"error": "human readable message"}`.
Times are RFC3339 strings. Shas are full 40-hex; `short_sha` is 12 chars.
All read shapes below are served from the in-memory **fold** of each
chain's event log (docs/data-model.md); the wire shapes are unchanged by
that — only `events`/`log` expose the log directly. Concurrency guarantees:
docs/data-model.md ("Concurrency", normative).

## Health

- `GET /api/health` → `{"status":"ok","version":"0.1.0"}`

## Chains

- `POST /api/chains` — register or refresh (idempotent; this is `nit push`)
  ```json
  req:  {"repo_path": "/abs/path", "branch": "feat/x", "base": "main",
         "partial": true}
  resp: Chain (below)
  ```
  `repo_path` is canonicalized; the chain row is auto-created. 400 if the
  repo/branch/base can't be resolved at registration. A scan failure on an
  _existing_ chain is not an HTTP error: it appears as `last_scan_error`.
  Every commit in `base..tip` must carry its own `Change-Id:` trailer and
  must not be a `fixup!`/`squash!` commit — violations fail the scan
  (docs/data-model.md "Change identity"). A scan that changes structure
  appends one `revisions` log entry.
  `partial` is optional and sticky across pushes: `true` marks the chain
  partial (`nit push --partial`), `false` clears it (`nit ready`),
  absent leaves it unchanged. A flip appends a `partial` entry. New chains
  default to not-partial.
- `GET /api/chains?status=active` → `{"chains": [Chain]}` — dashboard.
  Runs (throttled) scans first; a chain whose scan fails is still listed
  with its previous state plus `last_scan_error`. `status` defaults to
  `active`; `all` includes merged/abandoned.
- `GET /api/chains/{id}` → Chain (throttled rescan first). 404 if unknown.

```json
Chain = {
  "id": 1, "repo_path": "/abs/path", "branch": "feat/x", "base": "main",
  "status": "active",            // active | merged | abandoned
  "state": "waiting_for_review", // derived — see state table below
  "partial": false,              // sticky; set by push --partial, cleared by ready
  "last_scan_error": null,       // string when the last scan failed
  "web_url": "http://127.0.0.1:8877/chains/1",
  "created_at": "...", "updated_at": "...",  // updated_at = last entry's time
  "changes": [ChangeSummary]     // chain order; orphaned ones last
}
ChangeSummary = {
  "id": 10, "position": 0,           // null while orphaned
  "change_key": "I3f2…",
  "subject": "server: add health endpoint",
  "status": "pending",  // pending | approved | changes_requested
                        // | commented | orphaned
  "revision": 2,                 // latest revision number
  "last_reviewed_revision": 1,   // max revision with a review; null if none
  "commit_sha": "…", "short_sha": "abc123def456",
  "counts": {"revisions": 2, "published_comments": 3, "drafts": 1,
             "unresolved": 2}
}
```

`id` on a change is the fold-assigned change id (stable across the
chain's life); `comment` ids below are likewise fold-assigned
(docs/data-model.md "Identity within the log").

## Changes

- `GET /api/changes/{id}` — the change with every revision and every
  comment; each comment carries its own anchor verbatim (no `revision`
  query — placement is the client's job, see "Comment placement").
  ```json
  {
    "id": 10, "chain_id": 1, "change_key": "I3f2…", "position": 0,
    "status": "pending", "subject": "…",
    "last_reviewed_revision": 1,
    "revisions": [Revision],         // ascending
    "comments": [Comment],           // published + drafts, all revisions
    "reviews": [Review]
  }
  Revision = {"number": 2, "commit_sha": "…", "short_sha": "…",
              "parent_sha": "…", "message": "full commit message\n…",
              "created_at": "…"}
  Review   = {"id": 5, "revision": 2, "verdict": "request_changes",
              "message": "cover message", "created_at": "…"}
  ```
- `GET /api/changes/{id}/revisions/{n}/diff` → Diff of revision n against
  its parent.
- `GET /api/changes/{id}/revisions/{n}/diff?against={m}` → interdiff
  (revision m's tree → revision n's).

```json
Diff = {"files": [DiffFile]}
DiffFile = {
  "path": "src/main.rs",        // new path (old path when deleted)
  "old_path": "src/old.rs",     // only set for renames
  "status": "modified",         // added | deleted | modified | renamed
  "binary": false, "additions": 10, "deletions": 3,
  "hunks": [Hunk]               // empty when binary
}
Hunk = {"old_start": 1, "old_lines": 5, "new_start": 1, "new_lines": 7,
        "header": "fn main()",  // "" when there is no enclosing context
        "lines": [Line]}
Line = {"kind": "context",      // context | add | del
        "old": 1,               // old line number; absent for add
        "new": 1,               // new line number; absent for del
        "text": "fn main() {"}  // without trailing newline
```

### The commit message as a file

Every diff response lists the synthetic path `/COMMIT_MSG` as its
**first** file: the revision's full commit message, reviewable like code.
The path is reserved — git tree paths cannot start with `/`, so it can
never collide with a real file.

- vs parent: `status: "added"`, the whole message as one all-`add` hunk;
  `new` line numbers are 1-based message lines;
- interdiff (`?against=m`): `status: "modified"`, a real line diff of
  message(m) → message(n); identical messages yield one all-`context`
  hunk so the message stays visible and commentable;
- `binary` is always `false`, `old_path` never set; `additions`/
  `deletions` count message lines like any text file.

Line comments on `/COMMIT_MSG` use `side: "new"` only; old-side drafts
are rejected with 400. No UI state can produce one: vs parent the message
renders all-`add` (no old-side lines to select), and the old column of an
interdiff `m → n` shows message(m) — selecting there anchors a `new`-side
comment on revision `m` (the same mapping code uses, see drafts below) —
so a `/COMMIT_MSG` `side: "old"` anchor never arises; the 400 only guards
raw API clients.

### Comment placement

A line comment is anchored where it was written: a `revision`, a `side`,
a `line`, an optional `range`, and a `line_text` snapshot. The two sides
name trees of that revision:

- `side: "new"` → the line lives in the commit tree of `revision`;
- `side: "old"` → it lives in that revision's **parent** tree — the
  "before" of the revision's vs-parent diff, where deleted/old lines are.

A diff is always a range `FROM → TO`: `TO` is a revision `rN` (the right
select), `FROM` is `base` (its parent) or an earlier `rM` (the left
select, an interdiff). A comment shows **only when its `(revision, side)`
names one of the two displayed trees**, at its stored `line` — comments
are pinned to their patchset, never ported onto another revision:

| anchor      | shows when                    | side  |
| ----------- | ----------------------------- | ----- |
| `(rN, new)` | `TO == rN`                    | right |
| `(rN, old)` | `TO == rN` and `FROM == base` | left  |
| `(rM, new)` | `FROM == rM` (interdiff)      | left  |

A comment whose revision is neither `FROM` nor `TO` is **not shown in
that diff** (select its revision to see it). The old column of an
interdiff `rM → rN` shows `rM`'s own tree, so a comment made on `rM`'s
`new` side is what renders there on the left — there is no separate
"old" anchor for an interdiff. The `range` and `line` are served exactly
as written and read directly against the matching column.

A shown comment whose `line` lies outside the diff's rendered hunks (its
tree is displayed, but the line is in an unchanged region no hunk reaches)
groups per file with its `line_text` excerpt instead of rendering inline.

```json
Comment = {"id": 7, "change_id": 10, "revision": 2, "parent_id": null,
           "author": "reviewer",         // reviewer | agent
           "file": "src/main.rs",        // null: change-level comment
           "line": 14,                   // null: file-/change-level
           "side": "new",                // old | new (trees above)
           "range": CommentRange,        // null: whole-line comment
           "line_text": "    let x = parse(input);",  // null without line
           "body": "…", "state": "draft",   // draft | published
           "resolved": false,               // thread resolution (see below)
           "review_id": null, "created_at": "…", "updated_at": "…"}
```

`resolved` carries the thread's resolution, but reads differently per
comment: on a **published root** it is the thread's current state; on a
**published reply** it is always `false` (a thread's state lives on its
root); on a **draft** it is the resolution the reviewer has staged on that
comment's resolve checkbox, applied to the thread only when the draft
publishes (see "Thread resolution" below).

A published comment's `id`, `parent_id`, and `review_id` are fold-assigned
ids from the log; a draft's `id` is its row id in the `drafts` table.

### Range comments

A line comment may carry a `range` — the selected text it anchors to,
gerrit-style:

```json
CommentRange = {"start_line": 12, "start_char": 4,
                "end_line": 14, "end_char": 7}
```

- Lines are 1-based on the comment's `side`; chars are 0-based offsets
  into the line text, `end_char` exclusive.
- `end_line` equals the comment's `line` (the thread renders under the
  selection's last line) and the range is non-empty and forward:
  `start_line < end_line`, or `start_line == end_line` with
  `start_char < end_char`; `end_char >= 1` always (a selection ending
  before a line's first character belongs to the previous line).
  Violations → 400.
- Char offsets are not validated against file contents (the repo may not
  even be readable at draft time); the UI clamps when rendering.

A range is shown on whichever diff column its `(revision, side)` maps to
("Comment placement"), read directly against that column's line text — it
is never ported, because a comment only renders where its own tree is the
one displayed.

## Comments (drafts → published) — reviewer side

Drafts are reviewer-private scratch in their own table; they never enter
the log. Submitting a review drains a change's drafts into one `review`
log entry and deletes the rows (docs/data-model.md).

- `POST /api/changes/{id}/drafts` →
  `req: {"revision": 2, "file": "src/main.rs", "line": 14, "side": "new", "range": CommentRange, "body": "…", "parent_id": null, "resolved": false}`
  → Comment. `file`/`line` optional (change-/file-level). `side` defaults
  `"new"`. `range` optional: requires a `line` and must satisfy the
  "Range comments" rules, else 400. `file` may be the reserved
  `/COMMIT_MSG` (commit-message comments; `side` must be `"new"`, else
  400). `parent_id` references a published comment id (reply draft).
  `resolved` optional (default unset): the thread-resolution decision
  staged on this draft (see "Thread resolution"). A reply draft may carry
  an empty `body` when it stages a resolution change alone.
  Both columns of a diff are commentable: a new-column anchor stores
  `(revision = TO, side = "new")`; an old-column anchor stores
  `(revision = TO, side = "old")` against `base`, or `(revision = FROM,
side = "new")` in an interdiff (its old column is the FROM revision's own
  tree). The UI does this mapping; the endpoint just stores what it is sent.
- `PATCH /api/drafts/{id}` — `{"body": "…", "resolved": false}` → Comment.
  `resolved` optional. 404 unless draft.
- `DELETE /api/drafts/{id}` → 204. 404 unless draft.

### Thread resolution

A thread's resolved/unresolved state is **drafted, never immediate**
(gerrit-style): the reviewer stages it on a draft's resolve checkbox and it
takes effect when the review publishes. There is no resolve/unresolve
endpoint. The reply, resolve and reopen actions all save a draft (a reply
draft under the published root) carrying `resolved`; "reopen" stages
`false`, "resolve" `true`, a plain reply the thread's current state.

When the review publishes ("Reviews" below), each published comment carries
its staged `resolved` (`null` = no decision), applied to its thread in
payload order — so a thread ends at the **last** decision among the
published comments (data-model.md "The fold"). An empty-body draft that
only stages a resolution change applies its decision without adding a
visible comment. The agent stages resolution the same way through replies
(`nit reply --resolve` / `--unresolve`, below).

## Reviews

- `POST /api/changes/{id}/reviews` —
  `req: {"revision": 2, "verdict": "approve" | "request_changes" | "comment", "message": "…"}`
  Under the chain lock: drains the change's drafts (their staged `resolved`
  decisions included), appends one `review` log entry (verdict + cover
  message + the published comments), folds it (change status → the
  verdict's; each thread's resolution → its last decision), and emits it on
  the `/events` stream — no server-side relevance judgement
  (docs/data-model.md "Wake rule").
  - If `revision` is no longer latest but the latest is **patch-id-equal
    with an unchanged commit message** (pure rebase), the review
    auto-retargets to the latest revision and succeeds.
  - Otherwise stale `revision` → 409; the UI must keep the cover message
    and drafts, refetch, and re-offer submission.

  → `{"review": Review, "published_comments": [Comment]}` —
  `published_comments` omits any empty-body resolution-only draft (it
  changes a thread's state without becoming a comment).

## Agent endpoints

The agent drives the loop with a **0-based cursor** it owns: the count of
log entries it has already consumed. It never learns the cursor from a
mutating call (`push`/`reply` return no index) — only `events`/`log`
advance it — so an entry that lands between two of its own actions can't
be skipped (docs/agent-workflow.md).

- `POST /api/comments/{id}/replies` —
  `req: {"body": "…", "resolved": true}` → Comment (author=agent, published
  immediately, threaded under the root comment). `resolved` is the
  thread-resolution decision: `true` resolves, `false` reopens, omitted
  leaves it unchanged. Appends a one-element `reply` log entry. Used by
  `nit reply` (`--resolve` / `--unresolve`).
- `GET /api/chains/{id}/feedback` → Feedback (current fold, no blocking):
  ```json
  Feedback = {
    "state": "agents_turn",   // see state table
    "actionable": true,
    "chain": {"id": 1, "branch": "feat/x", "base": "main", "web_url": "…",
              "partial": false, "last_scan_error": null},
    "changes": [               // live changes, chain order
      {"change_id": 10, "change_key": "I3f2…", "subject": "…",
       "commit_sha": "…", "revision": 2, "status": "changes_requested",
       "unresolved": 2,
       "review": {"verdict": "request_changes", "message": "…",
                  "revision": 2},          // latest review, null if none
       "comments": [Comment]}              // that review's comments only,
    ]                                      // plus still-unresolved threads
  }                                        // from earlier reviews
  ```
- `GET /api/chains/{id}/events?cursor={c}` — a **Server-Sent Events**
  stream of the chain's log from `cursor` onward (the source behind
  `nit wait` and `nit log --follow`). `cursor` is the agent's 0-based offset (optional, defaults to
  `0` = from the start; an agent always passes its tracked cursor). On
  connect the server immediately replays every entry already
  past `cursor` (the "missed" backlog), then streams each new entry as it is
  appended; keep-alive comments hold the connection open while the chain is
  quiet. Each event's `data` is one `LogEntry`:
  ```
  data: {"idx": 5, "kind": "review", "created_at": "…", "payload": {…}}
  ```
  The stream is **raw**: the server emits every entry and makes no
  relevance judgement. Deciding which events matter — the **wake rule** — is
  the client's job (data-model.md), so one endpoint serves `nit wait`,
  `nit log --follow`, and a future event-driven UI. There is no timeout and no server-side
  filtering; the stream ends only on graceful shutdown or client
  disconnect, and a client resumes by reconnecting with its advanced
  `cursor` (nothing is skipped — the backlog replay covers the gap). The
  agent-side `{head, entries, feedback}` view of `nit wait` is assembled by
  the client from this stream plus `…/feedback`, not returned by the server.
- `GET /api/chains/{id}/log?from={a}&to={b}` → `{"head": 7, "entries":
[LogEntry]}` — the entries in `[from, to)` (both optional: `from`
  defaults 0, `to` defaults `head`). Read-only; never advances any cursor.
  References past the dataset are a **400**, not a silent clamp — a `to`
  beyond `head`, an open `from` beyond `head`, or a reversed/empty range
  (`to <= from`). A valid range that selects nothing (an open
  `from == head`) returns an empty list. Behind `nit log`.

```json
LogEntry = {
  "idx": 5,                 // 0-based position in the chain's log
  "kind": "review",         // revisions | review | reply | partial | chain_closed
  "created_at": "…",
  "payload": { … }          // kind-specific; shapes in data-model.md "Payloads"
}
```

The API ships only the raw entry. The one-line digest behind `--oneline`
is **not** an API field: it is a client display concern, derived from
`kind` + `payload` on demand (in the CLI), so its wording can change
without an API change and each client renders entries however it likes.

### State table (normative)

| state                | meaning                                                                                                                                                                | actionable |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| `waiting_for_review` | reviewer's turn; nothing for the agent                                                                                                                                 | false      |
| `agents_turn`        | request_changes/commented on a latest revision, empty chain, or all approved while `partial` (agent still pushing — `approved` is inexpressible while the flag is set) | true       |
| `approved`           | every live change approved (≥1) and the chain is not `partial`                                                                                                         | true       |
| `merged`             | chain closed: work is in the base                                                                                                                                      | true       |
| `abandoned`          | chain closed: branch disappeared                                                                                                                                       | true       |

`actionable` ≡ `state != waiting_for_review`. `state` is informational on
a `nit wait` return — the agent acts on the `entries` it received and the
state together (docs/agent-workflow.md).

## Static UI

Everything outside `/api` serves the built SPA (`--web-dist`/`$NIT_WEB_DIST`),
falling back to `index.html` for client-side routes (`/chains/1`,
`/changes/10`).
