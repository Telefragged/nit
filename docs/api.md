# HTTP API — the contract

Everything under `/api`, JSON in/out. **This file is the single source of
truth for shapes**: the frontend mirrors it in `web/src/api/types.ts`, the
backend in `crates/nit/src/api/types.rs`. Change shapes here first.

Errors: non-2xx with `{"error": "human readable message"}`.
Times are RFC3339 strings. Shas are full 40-hex; `short_sha` is 12 chars.
Concurrency guarantees behind these endpoints: docs/data-model.md
("Concurrency", normative).

## Health
- `GET /api/health` → `{"status":"ok","version":"0.1.0"}`

## Chains
- `POST /api/chains` — register or refresh (idempotent; this is `nit push`)
  ```json
  req:  {"repo_path": "/abs/path", "branch": "feat/x", "base": "main",
         "partial": true}
  resp: Chain (below)
  ```
  `repo_path` is canonicalized; the repo row is auto-created. 400 if the
  repo/branch/base can't be resolved at registration. A scan failure on an
  *existing* chain is not an HTTP error: it appears as `last_scan_error`.
  `partial` is optional and sticky across pushes: `true` marks the chain
  partial (`nit push --partial`), `false` clears it (`nit ready`),
  absent leaves it unchanged. New chains default to not-partial.
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
  "scan_warnings": [],           // e.g. duplicate Change-Id, squash! seen
  "web_url": "http://127.0.0.1:8877/chains/1",
  "created_at": "...", "updated_at": "...",
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
  "needs_rebase": false,         // fixup folding conflicted
  "counts": {"revisions": 2, "published_comments": 3, "drafts": 1,
             "unresolved": 2}
}
```

## Changes
- `GET /api/changes/{id}?revision={n}` — `revision` defaults to latest and
  controls comment rendering (below).
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
              "fixups": [{"sha": "…", "short_sha": "…", "message": "…"}],
              "needs_rebase": false, "created_at": "…"}
  Review   = {"id": 5, "revision": 2, "verdict": "request_changes",
              "message": "cover message", "created_at": "…"}
  ```
- `GET /api/changes/{id}/revisions/{n}/diff` → Diff of revision n against
  its parent. 409 if the revision `needs_rebase`.
- `GET /api/changes/{id}/revisions/{n}/diff?against={m}` → interdiff
  (revision m's effective tree → revision n's). 409 if either side
  `needs_rebase`.

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
are rejected with 400. No UI state can produce one — vs parent the
message renders all-`add` (no old-side lines exist), and the `modified`
presentation only occurs in interdiffs, where old-side commenting is
unsupported for every file in v1 (see drafts below) — so the 400 only
guards raw API clients. Anchors port across revisions by diffing the two
revisions' message texts, not trees (see "Comment rendering across
revisions").

### Comment rendering across revisions

Comments are anchored where they were written (`revision`, `file`, `line`,
`side`, plus a `line_text` snapshot). When the change is served at
`?revision=n`, the server ports each comment's anchor through
`diff(effective_tree(comment.revision), effective_tree(n))` per file. For
`/COMMIT_MSG` anchors the server ports through
`diff(message(comment.revision), message(n))` instead — same
shifted/outdated rules:

- line lies in an unchanged region → `rendered_line` = shifted line number;
- the anchored line itself was changed/deleted (or porting is impossible) →
  `rendered_line: null, outdated: true` — the UI shows it with `line_text`
  in an "outdated" group per file.

```json
Comment = {"id": 7, "change_id": 10, "revision": 2, "parent_id": null,
           "author": "reviewer",         // reviewer | agent
           "file": "src/main.rs",        // null: change-level comment
           "line": 14,                   // null: file-/change-level
           "side": "new",
           "line_text": "    let x = parse(input);",  // null without line
           "rendered_line": 14,          // for the requested revision
           "outdated": false,
           "body": "…", "state": "draft",   // draft | published
           "resolved": false,
           "review_id": null, "created_at": "…", "updated_at": "…"}
```

## Comments (drafts → published) — reviewer side
- `POST /api/changes/{id}/drafts` →
  `req: {"revision": 2, "file": "src/main.rs", "line": 14, "side": "new", "body": "…", "parent_id": null}`
  → Comment. `file`/`line` optional (change-/file-level). `side` defaults
  `"new"`. `file` may be the reserved `/COMMIT_MSG` (commit-message
  comments; `side` must be `"new"`, else 400). In interdiff view the UI
  may only attach comments to the *new* side; old-side interdiff
  commenting is unsupported in v1.
- `PATCH /api/drafts/{id}` — `{"body": "…"}` → Comment. 404 unless draft.
- `DELETE /api/drafts/{id}` → 204. 404 unless draft.
- `POST /api/comments/{id}/resolve` / `…/unresolve` → Comment (root
  comments only; reviewer toggling thread resolution).

## Reviews
- `POST /api/changes/{id}/reviews` —
  `req: {"revision": 2, "verdict": "approve" | "request_changes" | "comment", "message": "…"}`
  Under the chain lock: publishes **all** drafts on the change, creates the
  Review, updates change status (`approve`→approved,
  `request_changes`→changes_requested, `comment`→commented), emits
  `review_submitted`.
  - If `revision` is no longer latest but the latest is **patch-id-equal
    with the same fixups and an unchanged commit message** (pure rebase),
    the review auto-retargets to the latest revision and succeeds.
  - Otherwise stale `revision` → 409; the UI must keep the cover message
    and drafts, refetch, and re-offer submission.
  → `{"review": Review, "published_comments": [Comment]}`

## Agent endpoints
- `POST /api/comments/{id}/replies` —
  `req: {"body": "…", "resolve": true}` → Comment (author=agent, published
  immediately, threaded under the root comment; `resolve` marks the thread
  resolved). Emits `comment_replied`. Used by `nit reply`.
- `GET /api/chains/{id}/feedback` → Feedback (current state, no blocking):
  ```json
  Feedback = {
    "state": "agents_turn",   // see state table
    "actionable": true,
    "chain": {"id": 1, "branch": "feat/x", "base": "main", "web_url": "…",
              "partial": false, "last_scan_error": null, "scan_warnings": []},
    "changes": [               // live changes, chain order
      {"change_id": 10, "change_key": "I3f2…", "subject": "…",
       "commit_sha": "…", "revision": 2, "status": "changes_requested",
       "needs_rebase": false, "unresolved": 2,
       "review": {"verdict": "request_changes", "message": "…",
                  "revision": 2},          // latest review, null if none
       "comments": [Comment]}              // that review's comments only,
    ]                                      // plus still-unresolved threads
  }                                        // from earlier reviews
  ```
- `GET /api/chains/{id}/wait?cursor={c}&timeout={secs}` — long-poll, the
  one atomic call behind `nit wait`. Blocks until an event with id > c
  exists for this chain (or timeout, default 55, max 120), then returns
  `{"cursor": <latest event id>, "feedback": Feedback}` — cursor first
  obtained by calling with `cursor=0` (which returns immediately with the
  current snapshot). Clients must decide on `feedback`, never on the events
  themselves; re-poll with the returned cursor. The server may also return
  before the timeout with an unchanged snapshot (e.g. while shutting down);
  clients just re-poll with the returned cursor.

### State table (normative)

| state                | meaning                                   | actionable |
|----------------------|-------------------------------------------|------------|
| `waiting_for_review` | reviewer's turn; nothing for the agent    | false      |
| `agents_turn`        | request_changes/commented/needs_rebase on a latest revision, empty chain, or all approved while `partial` (agent still pushing — `ready_to_merge` is inexpressible while the flag is set) | true |
| `ready_to_merge`     | every live change approved (≥1) and the chain is not `partial` | true |
| `merged`             | chain closed: work is in the base         | true       |
| `abandoned`          | chain closed: branch disappeared          | true       |

`actionable` ≡ `state != waiting_for_review`. Every `actionable=true`
state has a documented agent action (agent-workflow.md).

## Static UI
Everything outside `/api` serves the built SPA (`--web-dist`/`$NIT_WEB_DIST`),
falling back to `index.html` for client-side routes (`/chains/1`,
`/changes/10`).
