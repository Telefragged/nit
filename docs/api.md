# HTTP API — the contract

Everything under `/api`, JSON in/out. **This file is the single source of
truth for shapes**: the frontend mirrors it in `web/src/api/types.ts`, the
backend in `crates/nit/src/api/types.rs`. Change shapes here first.

Errors: non-2xx with `{"error": "human readable message"}`.
Times are RFC3339 strings. Shas are full 40-hex; `short_sha` is 12 chars.

## Health
- `GET /api/health` → `{"status":"ok","version":"0.1.0"}`

## Chains
- `POST /api/chains` — register or refresh (idempotent; this is `nit push`)
  ```json
  req:  {"repo_path": "/abs/path", "branch": "feat/x", "base": "main"}
  resp: Chain (below)
  ```
  `repo_path` is canonicalized; the repo row is auto-created. 400 if the
  repo/branch/base can't be resolved.
- `GET /api/chains?status=active` → `{"chains": [Chain]}` — dashboard.
  Rescans every listed chain first. `status` defaults to `active`;
  `all` includes merged/abandoned.
- `GET /api/chains/{id}` → Chain (rescans first). 404 if unknown.

```json
Chain = {
  "id": 1, "repo_path": "/abs/path", "branch": "feat/x", "base": "main",
  "status": "active",            // active | merged | abandoned
  "state": "waiting_for_review", // derived: waiting_for_review | agents_turn | ready_to_merge
  "web_url": "http://127.0.0.1:8877/chains/1",
  "created_at": "...", "updated_at": "...",
  "changes": [ChangeSummary]     // chain order, oldest first
}
ChangeSummary = {
  "id": 10, "position": 0, "change_key": "I3f2…",
  "subject": "server: add health endpoint",
  "status": "pending",           // pending | approved | changes_requested
  "revision": 2,                 // latest revision number
  "commit_sha": "…", "short_sha": "abc123def456",
  "needs_rebase": false,         // fixup folding conflicted
  "counts": {"revisions": 2, "published_comments": 3, "drafts": 1}
}
```

## Changes
- `GET /api/changes/{id}` →
  ```json
  {
    "id": 10, "chain_id": 1, "change_key": "I3f2…", "position": 0,
    "status": "pending", "subject": "…",
    "revisions": [Revision],         // ascending
    "comments": [Comment],           // published + drafts, all revisions
    "reviews": [Review]
  }
  Revision = {"number": 2, "commit_sha": "…", "short_sha": "…",
              "parent_sha": "…", "message": "full commit message\n…",
              "fixup_shas": ["…"], "needs_rebase": false, "created_at": "…"}
  Review   = {"id": 5, "revision": 2, "verdict": "request_changes",
              "message": "cover message", "created_at": "…"}
  ```
- `GET /api/changes/{id}/revisions/{n}/diff` → Diff of revision n against its
  parent.
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
        "header": "fn main()", "lines": [Line]}
Line = {"kind": "context",      // context | add | del
        "old": 1,               // old line number; absent for add
        "new": 1,               // new line number; absent for del
        "text": "fn main() {"}  // without trailing newline
```

## Comments (drafts → published)
- `POST /api/changes/{id}/drafts` →
  `req: {"revision": 2, "file": "src/main.rs", "line": 14, "side": "new", "body": "…"}`
  → Comment. `file`/`line` optional (change-/file-level). `side` defaults `"new"`,
  meaningful only with `line` (`"old"` anchors to the deleted side).
- `PATCH /api/drafts/{id}` — `{"body": "…"}` → Comment. 404 unless draft.
- `DELETE /api/drafts/{id}` → 204. 404 unless draft.

```json
Comment = {"id": 7, "change_id": 10, "revision": 2,
           "file": "src/main.rs", "line": 14, "side": "new",
           "body": "…", "state": "draft",   // draft | published
           "review_id": null, "created_at": "…", "updated_at": "…"}
```

## Reviews
- `POST /api/changes/{id}/reviews` —
  `req: {"revision": 2, "verdict": "approve" | "request_changes" | "comment", "message": "…"}`
  Publishes **all** drafts on the change, creates the Review, updates change
  status, emits `review_submitted`. 409 if `revision` is no longer the
  latest (the agent pushed meanwhile — reviewer must reload).
  → `{"review": Review, "published_comments": [Comment]}`

## Agent endpoints
- `GET /api/chains/{id}/feedback` → current actionable state (what `nit wait`
  prints; see agent-workflow.md for the exact JSON).
  `"actionable"` is true when any change of the latest revisions has a
  non-approve review, or every change is approved, or the chain is
  merged/abandoned.
- `GET /api/chains/{id}/events?after={cursor}&timeout={secs}` — long-poll.
  Returns `{"events":[{"id":42,"kind":"review_submitted","payload":{…},
  "created_at":"…"}], "cursor": 42}` as soon as events with id > cursor
  exist, else `{"events":[],"cursor":<cursor>}` after timeout (default 55,
  max 120). `after=0` returns all stored events for the chain.

## Static UI
Everything outside `/api` serves the built SPA (`--web-dist`/`$NIT_WEB_DIST`),
falling back to `index.html` for client-side routes (`/chains/1`,
`/changes/10`).
