## Changes

- `GET /api/changes/{id}` — the change with every revision, every comment
  thread, and the reviewer's open drafts. Each thread carries its anchor
  verbatim (no `revision` query — placement is the client's job, see "Comment
  placement").
  ```json
  {
    "id": 10, "repo_id": 1, "change_key": "I3f2…",
    "revisions": [Revision],         // ascending
    "threads": [Thread],             // published threads, all revisions
    "drafts": [Draft],               // reviewer's unpublished comments
    "reviews": [Review],             // each carries its revision
    "draft_decision": StagedDecision // the reviewer's staged decision, or null
  }
  Revision = {"number": 2, "commit_sha": "…",
              "parent_sha": "…", "base_sha": "…",
              "partial": false, "message": "full commit message\n…",
              "created_at": "…"}
  Review   = {"id": 5, "revision": 2, "verdict": "request_changes",
              "message": "cover message", "created_at": "…"}
  StagedDecision = {"decision": "approve",   // Decision: approve | request_changes
                    "message": "cover note"} //   | comment | abandon | reopen
  ```
  There is no `chain_id` or `position` — both are properties of a path, not of
  the change; a `PathEntry` from `GET /api/chains/{id}` carries them.
  `reviews` and `threads` are change-wide and carry their `revision`; a client
  viewing one patchset MUST filter by the viewing `?revision`.
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
        "drift": false,         // changed by a rebase, not the agent (omitted
                                // when false; see "Rebase-aware interdiffs")
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

Line comments on `/COMMIT_MSG` use `side: "new"` only; old-side drafts are
rejected with 400.

### Rebase-aware interdiffs

An interdiff `m → n` is `tree(m) → tree(n)`. When `m` and `n` have
**different parents** (the base moved, or an earlier change in the chain
got a revision, rewriting every later one), the gap between the two parents
folds into the interdiff alongside the agent's real edits. nit detects that
**drift** and contains it (gerrit's "due to rebase"), so the reviewer is
not shown base movement they did not make.

A diff against parent, a same-parent interdiff, and `/COMMIT_MSG` are never
drift-processed — they are the plain diff byte-for-byte. When
`parent(m) != parent(n)`, each non-binary code file is classified:

- **Detection.** Diff the two parents (`parent(m) → parent(n)`) and project
  those edits into `m`/`n` coordinates through the change's own delta at
  each revision, so a base edit is recognised wherever the agent's edits
  shifted it; lines the agent also touched are clipped out and show as real.
  Matching is **line-level**, with two gerrit-like limitations (no
  intraline/move detection): on runs of identical lines some base movement
  can show as a real change (the safe direction), and a base _reorder_ of a
  line the agent also deletes can tag that deletion as drift.
- **`drift: true`** marks each base-movement line; the UI tints them,
  otherwise they are ordinary lines.
- **Counts exclude drift** — `additions`/`deletions` count only non-drift
  `add`/`del` lines.
- **Region selection follows the real delta.** A hunk shows because it
  carries a real edit; drift renders only inside such a hunk. An all-drift
  hunk is omitted, and an all-drift file drops out entirely (so a pure
  rebase of a change collapses to just its `/COMMIT_MSG`).
- **Renamed/copied files are not drift-processed**; their edits all render
  as real.

`parent(m) → parent(n)` for a change is exactly its **parent change's** own
`m → n` interdiff — down a stack each change subtracts its parent's movement.

### Comment placement

A thread is anchored where its first comment was written: a `revision`, a
`side`, a `line`, an optional `range`, and a `line_text` snapshot. The two
sides name trees of that revision:

- `side: "new"` → the line lives in the commit tree of `revision`;
- `side: "old"` → it lives in that revision's **parent** tree — the
  "before" of the revision's vs-parent diff, where deleted/old lines are.

A diff is always a range `FROM → TO`: `TO` is a revision `rN` (the right
select), `FROM` is `base` (its parent) or an earlier `rM` (the left
select, an interdiff). A thread shows **only when its `(revision, side)`
names one of the two displayed trees**, at its stored `line` — threads
are pinned to their patchset, never ported onto another revision:

| anchor      | shows when                    | side  |
| ----------- | ----------------------------- | ----- |
| `(rN, new)` | `TO == rN`                    | right |
| `(rN, old)` | `TO == rN` and `FROM == base` | left  |
| `(rM, new)` | `FROM == rM` (interdiff)      | left  |

A thread whose revision is neither `FROM` nor `TO` is **not shown in
that diff** (select its revision to see it). The old column of an
interdiff `rM → rN` shows `rM`'s own tree, so a thread anchored to `rM`'s
`new` side is what renders there on the left — there is no separate
"old" anchor for an interdiff. The `range` and `line` are served exactly
as written and read directly against the matching column.

A shown thread whose `line` lies outside the diff's rendered hunks (its
tree is displayed, but the line is in an unchanged region no hunk reaches)
groups per file with its `line_text` excerpt instead of rendering inline.

```json
Thread = {"id": 7, "change_id": 10, "revision": 2,
          "file": "src/main.rs",        // null: change-level
          "line": 14,                   // null: file-/change-level
          "side": "new",                // old | new (trees above)
          "range": CommentRange,        // null: whole-line
          "line_text": "    let x = parse(input);",  // null without line
          "resolved": false,            // the thread's rolled-up state
          "comments": [ThreadComment],  // chronological
          "created_at": "…", "updated_at": "…"}
ThreadComment = {"body": "…",
                 "review_id": 5,        // the review that published it; null for an agent comment — the client derives reviewer/agent from this
                 "created_at": "…"}
Draft = {"id": 31, "change_id": 10,     // a reviewer's unpublished comment
         "thread_id": 7,                // set: replies to that thread; null: opens a new one
         "revision": 2,                 // the request's anchor revision; only a new thread uses it (a reply keeps the thread's)
         "file": "src/main.rs", "line": 14, "side": "new",
         "range": CommentRange, "line_text": "…",
         "body": "…",                   // may be empty for a resolution-only reply draft
         "resolved": false,             // the staged thread decision (see "Thread resolution")
         "created_at": "…", "updated_at": "…"}
```

A thread's `id` is fold-assigned by fold order (not stored); its
`change_id` and a comment's `review_id` are fold ids from the log; a
draft's `id` is its row id in the `drafts` table. A thread is born from its
first comment — reviewer- **or** agent-initiated — so a thread whose
`comments[0].review_id` is `null` is a note the agent left on its own change,
and the reviewer engages with it exactly like any other (reply, resolve).

### Range comments

A thread may carry a `range` — the selected text it anchors to,
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
is never ported, because a thread only renders where its own tree is the
one displayed.
