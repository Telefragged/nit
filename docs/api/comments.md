## Comments (drafts → published) — reviewer side

Drafts are reviewer-private scratch in their own table; they never enter
the log. Submitting a review drains a change's drafts into one `review`
log entry and deletes the rows (docs/data-model.md).

- `POST /api/changes/{id}/drafts` →
  `req: {"revision": 2, "file": "src/main.rs", "line": 14, "side": "new", "range": CommentRange, "body": "…", "thread_id": null, "resolved": false}`
  → Draft. `file`/`line` optional (change-/file-level). `side` defaults
  `"new"`. `range` optional, mutually exclusive with `line` (the stored
  `line` becomes `range.end_line`); must satisfy the "Range comments"
  rules, else 400. `file` may be the reserved
  `/COMMIT_MSG` (commit-message comments; `side` must be `"new"`, else
  400). `thread_id` references an existing thread on this change (a reply
  draft); absent, the draft opens a new thread anchored by the fields
  above. `resolved` optional (default unset): the thread-resolution decision
  staged on this draft (see "Thread resolution"). A reply draft may carry
  an empty `body` when it stages a resolution change alone.
  Both columns of a diff are commentable: a new-column anchor stores
  `(revision = TO, side = "new")`; an old-column anchor stores
  `(revision = TO, side = "old")` against `base`, or `(revision = FROM,
side = "new")` in an interdiff (its old column is the FROM revision's own
  tree). The UI does this mapping; the endpoint just stores what it is sent.
- `PATCH /api/drafts/{id}` — `{"body": "…", "resolved": false}` → Draft.
  `resolved` optional. 404 unless draft.
- `DELETE /api/drafts/{id}` → 204. 404 unless draft.

### Thread resolution

A thread's resolved/unresolved state is **drafted, never immediate**
(gerrit-style): the reviewer stages it on a draft's resolve checkbox and it
takes effect when the review publishes. There is no resolve/unresolve
endpoint. The reply, resolve and reopen actions all save a draft (carrying
the thread's `thread_id`) with `resolved`; "reopen" stages `false`,
"resolve" `true`, a plain reply the thread's current state.

When the review publishes ("Reviews" below), each drained draft carries its
staged `resolved` (`null` = no decision), applied to its thread in draft
order — so a thread ends at the **last** decision among them (data-model.md
"The fold"). An empty-body draft that only stages a resolution change moves
the thread without adding a visible comment. An agent stages resolution the
same way, through `nit comment --thread <id> --resolve` / `--unresolve`
(below).
