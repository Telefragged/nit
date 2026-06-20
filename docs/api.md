# HTTP API — the contract

Everything under `/api`, JSON in/out. **This file is the single source of
truth for shapes**: the frontend mirrors it in `web/src/api/types.ts`, the
backend in `crates/nit/src/api/types.rs`. Change shapes here first.

Errors: non-2xx with `{"error": "human readable message"}`.
Times are RFC3339 strings. Shas are full 40-hex; clients truncate for
display (12 chars, the canonical short form).

The unit of state is the **change** (a `Change-Id`, scoped to a repo): it
owns an append-only log whose fold is its reviewable state
(docs/data-model.md). A **chain** is never stored — it is derived on demand
by walking a tip commit back to the repo's canonical branch through each
revision's recorded `parent_sha` (gerrit relation chains). Every read shape
below is served from the in-memory fold; chains are assembled at read time
from the per-change folds. Concurrency guarantees: docs/data-model.md
("Concurrency", normative).

A follower watches a **set** of changes over **one websocket** (`WS
/api/stream`, "Events"); one-shot reads (`nit status`, `nit log`) and the web
poll the same folds.

## Health

- `GET /api/health` → `{"status":"ok","version":"0.1.0"}`

## Repos

A repo is the registry grouping for changes; its identity is the
**git-common-dir** (the `.git` dir, shared across worktrees), which is also
its display name. A repo has exactly **one canonical branch** (`base_branch`)
— mergedness is always tracked against it, there is no land-anywhere. The web
main page lists repos, each linking to that repo's chains. Repos are created
lazily by the first `nit push`; there is no separate registration step.

- `GET /api/repos` → `{"repos": [Repo]}` — registration order.
- `GET /api/repos/{id}` → Repo — one repo by id; 404 if unknown.
- `PATCH /api/repos/{id}` — repoint a repo at a new git-common-dir after it
  moved on disk (`nit repo move`).
  ```json
  req:  {"git_dir": "/new/path/.git"}
  resp: Repo
  ```
  `git_dir` is canonicalized and must open as a git repo. 404 if the repo is
  unknown, 400 if the new path can't be resolved, 409 if it already belongs
  to another repo.

```json
Repo = {
  "id": 1,
  "git_dir": "/abs/path/.git",   // canonical git-common-dir — identity + name
  "base_branch": "main",         // the one canonical branch; mergedness tracks it
  "active_chains": 2             // live tip count (derived from the tip set)
}
```

## Push

- `POST /api/push` — register a tip for review (idempotent; this is
  `nit push`).

  ```json
  req:  {"git_dir": "/abs/path/.git", "tip": "f3a9…", "base": "main",
         "partial": true}
  resp: PushResult (below)
  ```

  `git_dir` is the repo's canonical **git-common-dir** (`git rev-parse
--git-common-dir`), canonicalized server-side; the `nit` CLI infers it from
  the cwd. `base` configures the repo's canonical branch: recorded on the
  repo's first push, it must equal the stored `base_branch` on every push
  after — a different base is a **400** (one canonical branch per repo).
  `base` is **optional**: omitted, a registered repo reuses its stored
  `base_branch`, and a fresh repo auto-detects the local `main` or `master`
  — a **400** asking the caller to specify `base` when neither or both exist.
  `tip` is any
  ref or rev, resolved to a commit at push time (the CLI sends the resolved
  commit sha of its checked-out HEAD by default); git is the source of truth
  for branch position, nit stores no branch sha.

  The server walks `merge-base(base, tip)..tip` oldest-first and, for each
  commit, **upserts the change** (keyed by its `Change-Id`) and **appends a
  `revision` entry iff the commit-sha moved** (a pure rebase — patch-id-equal
  with an unchanged message — appends a revision but does not reset review
  status). The walk is **all-or-nothing**: a `400` rejects the whole push on
  any structural fault (a merge or root commit, a commit missing its
  `Change-Id` trailer, a duplicate trailer within the walk, a `fixup!`/
  `squash!` subject, or a commit-sha already recorded under a different
  change). A push that would add a revision to an **abandoned** change is a
  **409** — reopen it first (`nit reopen`).

  `partial` is optional and sticky: `true` marks the tip's latest revision
  partial (`nit push --partial`), `false` clears it (`nit ready`), absent
  leaves it unchanged. A push that walks to nothing (`tip` is ancestor-or-equal
  of `base`) is a **409** — the tip is already merged into the base (or is the
  base itself), so there is nothing to review. A re-push where the walk is
  non-empty but nothing moved is **idempotent** (200), so a crash-retry is safe.

```json
PushResult = {
  "tip_change": {"change_id": 10, "change_key": "I3f2…",
                 "revision": 2, "status": "pending"},
  "chain": Chain    // tip-rooted: the derived path, each member at the
                    // revision this push gave it (see "Chains")
}
```

There is no chain id — a chain is addressed by its **tip change id** plus an
optional `?revision` selecting the patchset (and hence the chain context).

## Chains

A chain is the ordered path from the canonical branch up to a tip commit,
each member pinned to the patchset that tip walked through. Nothing about a
chain is stored: these endpoints compute it from the in-memory tip-commit set
and the commit-sha → `(change, revision)` index (docs/data-model.md "Chain
derivation").

- `GET /api/chains?repo={id}` → `{"chains": [ChainSummary]}` — one entry per
  known **tip commit** (the dashboard). `status` defaults to `active` (live
  tips — neither merged nor abandoned, so an abandoned tip is hidden here);
  `all` also includes merged and abandoned tips.
- `GET /api/chains/{change_id}` → Chain — the derived path through that
  change's tip commit. An **abandoned** change still resolves (it stays a
  member, and a tip if it is a leaf) — abandonment is membership-inert.
  `?revision={n}` selects which patchset of the change to root on (default: its
  latest); the selected revision's `parent_sha` determines the path, so
  `?revision` _is_ the choice of chain context. 404 if the change is unknown.
- `GET /api/chains/{change_id}/log` → the **aggregated** chain log: every
  member's log entries, merged and sorted by global `seq` (one timeline for
  the whole chain). Behind `nit log`.
- `POST /api/chains/{change_id}/submit` → BatchSubmitResult — **publish every
  member's staged decision** for this chain (docs/data-model.md "Reviewer
  decisions"). `?revision={n}` picks the chain context exactly like
  `GET /api/chains/{change_id}` (default: the change's latest). The path is
  re-derived at submit time; for each member carrying a staged decision it
  publishes that decision **at the revision this path pins on the member** (not
  a stored revision), in that member's own per-change transaction (atomic per
  change, **not** atomic across the chain — like `nit push`). A member with no
  staged decision is left untouched (its comment drafts stay drafts). The
  per-change publish deletes the member's `draft_reviews` row, so re-submitting
  finishes a torn batch without double-publishing — submit is idempotent.

  ```json
  resp: BatchSubmitResult = {
    "submitted": 2,                       // members whose decision published
    "errors": [SubmitError]               // members skipped (stale/terminal)
  }
  SubmitError = {"change_id": 11, "message": "change is abandoned — stage Reopen"}
  ```

```json
ChainSummary = {
  "tip_change_id": 12,
  "repo_id": 1,                  // the repo this chain belongs to
  "name": "feat/x",              // best-effort, resolved at query time (below)
  "state": "waiting_for_review", // derived — see state table
  "partial": false,              // the tip's latest revision is partial
  "updated_at": "…",             // newest member entry's time
  "path": [PathEntry]            // oldest-first, base → tip
}
Chain = {
  "tip_change_id": 12,
  "repo_id": 1,
  "name": "feat/x",
  "base_branch": "main",
  "state": "waiting_for_review",
  "partial": false,
  "path": [PathEntry]
}
PathEntry = {
  "change_id": 10, "position": 0,    // position is a property of THIS path
  "change_key": "I3f2…",
  "revision": 2,                     // the patchset this path walks
  "latest_revision": 3,              // newest patchset anywhere; > revision drives
                                     // the client's "newer elsewhere" badge
  "status": "pending",               // per (change, this revision)
  "merged_elsewhere": false,         // a newer revision landed on the canonical branch
  "subject": "server: add health endpoint",
  "commit_sha": "…",
  "counts": {"threads": 3, "drafts": 1, "unresolved": 2}, // scoped to this revision
  "draft_decision": "approve"        // the change's staged decision (Decision),
                                     // or null; change-wide (one per change),
                                     // so it shows on every chain the change is in
}
```

`position`, `status`, `unresolved`, and `state` are read **at the path's
pinned revision** — two tips placing the same change differently carry
independent verdicts (a request_changes in one chain never overwrites an
approve in another). `id` on a change is its stable fold id (the `change`
rowid); thread ids are fold-assigned by fold order (docs/data-model.md
"Identity").

`draft_decision` is the one exception to "read at the pinned revision": a draft
decision is **change-wide** (one per change), so the same value appears on every
chain the change is a member of. The dashboard counts a member as having
reviewer draft state when `counts.drafts > 0` **or** `draft_decision != null`,
and enables a chain's batch submit when any member carries a `draft_decision`.

### Tip names

A tip is named best-effort at query time (nit stores no branch key): a branch
ref that `git branch --contains <tip>` keeps stable as the agent advances,
else a tag, else the commit subject. A push that advances a tip keeps the same
name; deleting a branch only drops a name, not the tip.

### The B-in-two-chains example

Two pushes in one repo, canonical `main` at merge-base `m`:

- push 1: `m → A → B → C` (Change-Ids `Ia, Ib, Ic`)
- push 2: `m → D → B′ → E` (`Id, Ib, Ie`, B re-parented onto D)

`B` is one change with two patchsets: rev0 `parent=A`, rev1 `parent=D`. Two
tips, two chains: `chains/Ic` walks B at rev0, `chains/Ie` walks B at rev1.
Threads and reviews on B are **shared** (they belong to the change) and each
is anchored to the revision it was written against; `?revision` selects which
patchset — and chain context — you view.

## Graph

The repo's **change graph** is one spine-centered DAG over the canonical
branch — the source for the web dashboard, which replaces the per-chain
tables. Where `/api/chains` returns independent tip-rooted _paths_ that
duplicate a change shared by two chains, the graph is a single
commit-sha-keyed node set: a shared change appears **once**, and fan-out and
merge commits are first-class. Like a chain, nothing about it is stored — it
is assembled at read time from the same in-memory folds + sha index, plus a
git walk of the canonical branch for the merged history.

- `GET /api/repos/{id}/graph` → RepoGraph. 404 if the repo is unknown. The
  history region is a fixed window of merged commits below the canonical HEAD
  (5); there is no client knob — paging deeper is a future paginated endpoint,
  not a refetch of the whole graph.

The graph has three regions around the **canonical HEAD** anchor — resolved
live from `base_branch`, never assumed equal to any one chain's recorded
`base_sha` (each push computed its own merge-base):

- **open** — every active change ascending above HEAD, derived exactly like
  `/api/chains` (each active tip walked back to its fork) then unioned and
  **deduplicated by commit-sha**. Only a reachable revision is a node: an
  amended tip's superseded revision is unreachable and never appears. The rare
  B-in-two-chains case (one change live at two revisions under two tips) is
  two nodes — they are different commits with different parents, so collapsing
  them would break a descendant's lineage.
  An open change may fork **behind** HEAD — its base predates the current HEAD
  (the canonical branch advanced without a rebase). It keeps its real base
  `parents`; the client draws that as a distinct edge (to the base node when it
  is within the window, else down into the truncation marker below).
- **head** — the canonical HEAD commit, the anchor (one node).
- **history** — up to a fixed window (5) of merged commits descending below HEAD, a
  git walk of the canonical branch. A commit mapping to a known change (by its
  `Change-Id` trailer) is enriched with that `change_id`/`change_key`; a merge
  or pre-nit commit is a bare node (subject from the commit message, no
  change). `history_truncated` is true when the branch has more merged commits
  below the window — the client shows an "earlier history hidden" marker that
  the spine descends into and behind-forks older than the window dangle to.

Nodes are returned in **row order** (top → bottom): a topological order in
which every node precedes its parents (children ascend, parents descend), so
the array index _is_ the row. Each node lists its `parents` by commit-sha; the
client inverts these for fan-out, packs lanes (the canonical branch is the
pinned center column), and renders. An edge is drawn to whichever parents are
in the node set; `parents.len() > 1` is a merge.

```json
RepoGraph = {
  "repo_id": 1,
  "base_branch": "main",
  "anchor": "9f12c0a…",        // the head node's commit_sha
  "history_truncated": false,  // more merged commits exist below the window
  "nodes": [GraphNode]         // row order: open (top) → head → history (bottom)
}
GraphNode = {
  "commit_sha": "a1f7c0d…",    // 40-hex; the node's stable id; client truncates
  "section": "open",           // open | head | history
  "subject": "feat(api): rename the --base flag",
  "status": "pending",         // ChangeStatus at the node's revision; the client
                               //   styles by section (head/history render merged)
  "parents": ["7b0c784…"],     // parent commit-shas; edges to those present; len>1 is a merge
  "change_id": 10,             // null for a bare git commit (merge / pre-nit)
  "change_key": "I3f2…",       // null with change_id
  "revision": 2,               // the pinned patchset (open nodes); null off the open region
  "counts": {"threads": 3, "drafts": 1, "unresolved": 2}, // activity; zeros off the open region
  "draft_decision": "approve"  // the change's staged decision (Decision), or null
}
```

`status`, `counts`, and `draft_decision` are read at the node's pinned
revision, exactly as a `PathEntry`. `change_id`/`change_key`/`revision` are
null on a bare git commit, and `revision` is null on the head node.

## Changes

- `GET /api/changes/{id}` — the change with every revision, every comment
  thread, and the reviewer's open drafts. Each thread carries its anchor
  verbatim (no `revision` query — placement is the client's job, see "Comment
  placement").
  ```json
  {
    "id": 10, "repo_id": 1, "change_key": "I3f2…",
    "subject": "…",
    "revisions": [Revision],         // ascending
    "threads": [Thread],             // published threads, all revisions
    "drafts": [Draft],               // reviewer's unpublished comments
    "reviews": [Review],             // each carries its revision
    "chains": [ChainRef],            // every tip walking through this change
    "draft_decision": StagedDecision // the reviewer's staged decision, or null
  }
  Revision = {"number": 2, "commit_sha": "…",
              "parent_sha": "…", "base_sha": "…",
              "partial": false, "message": "full commit message\n…",
              "created_at": "…"}
  Review   = {"id": 5, "revision": 2, "verdict": "request_changes",
              "message": "cover message", "created_at": "…"}
  ChainRef = {"tip_change_id": 12, "revision": 2, "name": "feat/x"}
  StagedDecision = {"decision": "approve",   // Decision: approve | request_changes
                    "message": "cover note"} //   | comment | abandon | reopen
  ```
  There is no `chain_id` or `position` — both are properties of a path, not of
  the change; read them from `chains` / a `PathEntry`. `reviews` and `threads`
  are change-wide and carry their `revision`; a client viewing one patchset
  MUST filter by the viewing `?revision`.
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
ThreadComment = {"author": "reviewer",  // reviewer | agent
                 "body": "…",
                 "review_id": 5,        // the review that published it; null for an agent comment
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
`comments[0].author` is `agent` is a note the agent left on its own change,
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

## Comments (drafts → published) — reviewer side

Drafts are reviewer-private scratch in their own table; they never enter
the log. Submitting a review drains a change's drafts into one `review`
log entry and deletes the rows (docs/data-model.md).

- `POST /api/changes/{id}/drafts` →
  `req: {"revision": 2, "file": "src/main.rs", "line": 14, "side": "new", "range": CommentRange, "body": "…", "thread_id": null, "resolved": false}`
  → Draft. `file`/`line` optional (change-/file-level). `side` defaults
  `"new"`. `range` optional: requires a `line` and must satisfy the
  "Range comments" rules, else 400. `file` may be the reserved
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

## Reviewer decisions (drafts)

Like comment threads, a reviewer's **decision** on a change is **drafted, not
immediate** (docs/data-model.md "Reviewer decisions"): the review modal stages a
`Decision` and a cover message in the `draft_reviews` side table (one mutable
row per change, reviewer-private, never in the log), and the chain-level batch
submit (`POST /api/chains/{id}/submit`, "Chains") publishes every member's
staged decision at once. A `Decision` is a verdict (`approve`,
`request_changes`, `comment`) **or** a lifecycle action (`abandon`, `reopen`),
so the modal offers all of them in one place — abandonment is a decision, not a
separate button.

- `PUT /api/changes/{id}/decision` —
  `req: {"decision": "approve" | "request_changes" | "comment" | "abandon" | "reopen", "message": "…"}`
  → StagedDecision. Stages (or overwrites) the change's draft decision. `message`
  is optional (the cover note for a verdict, the reason for `abandon`). Validated
  only as an enum value — legality against the change's lifecycle is checked at
  submit time, since a draft is reviewer scratch. 404 if the change is unknown.
- `DELETE /api/changes/{id}/decision` → 204 — discard the staged decision. 404
  if the change is unknown (a no-op when nothing is staged is still 204).

On batch submit each staged decision publishes **at the revision its chain path
pins on the member** (the path is the authority — a change-wide decision row
stores no revision, so the B-in-two-chains member publishes at rev0 from one
chain and rev1 from the other): a verdict drains the change's comment drafts
(their staged `resolved` decisions included) into one `review` log entry, sets
the `(change, revision)` status to the verdict, and applies each thread's
resolution in draft order; `abandon`/`reopen` append a `lifecycle` entry, and
any comment drafts on the change still drain into a `comment` review in the
**same** per-change transaction so they are never stranded. A member whose
staged decision is illegal for its current lifecycle (a verdict on a
merged/abandoned change, a `reopen` on a live one) is skipped into
`BatchSubmitResult.errors` and keeps its row. Batch submit is the **only** way
a reviewer verdict reaches the log — there is no immediate single-change submit.

## Agent endpoints

The agent drives the loop with a per-change cursor it owns (a vector of
`change_id → idx`); `nit push`/`nit comment` return no index, so an entry
that lands between two of its own actions is never skipped
(docs/agent-workflow.md). One-shot reads (`nit status`, `nit log`) read the
cursor's gap; the live followers (`nit wait`, `nit log --follow`) drive it
over the websocket ("Events").

- `POST /api/changes/{id}/comments` —
  `req: {"thread_id": null, "revision": 2, "file": "Cargo.toml", "line": 14, "side": "new", "range": CommentRange, "body": "…", "resolved": false}`
  → Thread (author=agent, published immediately). The agent's single
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
  "kind": "review",         // revision | review | comment | lifecycle | partial
  "created_at": "…",
  "payload": { … }          // kind-specific; shapes in data-model.md "Payloads"
}
```

The API ships only the raw entry. The one-line digest behind `--oneline` is
**not** an API field: it is a client display concern, derived from `kind` +
`payload` on demand (in the CLI). The aggregated chain log
(`GET /api/chains/{change_id}/log`) returns these entries merged across the
chain's members and sorted by `seq`.

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

## Static UI

Everything outside `/api` serves the built SPA (`--web-dist`/`$NIT_WEB_DIST`),
falling back to `index.html` for client-side routes (`/repos/1`,
`/changes/10`).
