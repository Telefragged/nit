# Data model

Review state is an **append-only event log**: each chain owns a log, and
its entire reviewable state (changes, revisions, comment threads, reviews,
partial flag, open/closed status) is the **fold** of that log. Nothing is
mutated or deleted — a correction is a new entry. The server holds the fold
in memory, rebuilt by replaying the log on startup; SQLite stores only the
log plus three non-history side tables (repo registry, chain registration,
reviewer drafts). git objects stay in the user's repo (pinned — "GC
safety"); diffs are computed on demand from the commit shas an entry
carries, never stored ("Diffs").

## Tables

```sql
repos   (id, git_dir, UNIQUE(git_dir))
        -- repo registry: canonical git-common-dir → id, the grouping key
        -- chains hang off. Stores nothing derivable from git (no branches,
        -- bases, commits, timestamps — those live in .git). git_dir is the
        -- repo's identity *and* display name; `nit repo move` repoints it.

chains  (id, repo_id, branch, base, created_at, UNIQUE(repo_id, branch))
        -- registration identity, grouped under a repo. The repo's git_dir
        -- is the path every git op opens; all chain state (status, partial,
        -- changes, comments…) is folded from its log, never stored here.

log     (chain_id, idx, kind, payload, created_at,
         PRIMARY KEY (chain_id, idx))
        -- the append-only log. idx is 0-based and contiguous per chain.
        -- payload is kind-specific JSON (below). head(chain) = entry count
        -- = idx of the next entry to append.

drafts  (id, chain_id, change_key, revision, thread_id, file, line, side,
         range_start_line, range_start_char, range_end_line, range_end_char,
         line_text, body, resolved, created_at, updated_at)
        -- reviewer-private unpublished comments. Mutable (PATCH/DELETE),
        -- never in the log: publishing a review drains a change's drafts
        -- into one `review` entry and deletes the rows. thread_id set =
        -- reply to that thread; NULL = opens a new thread (file/line/side/
        -- range anchor). side: old | new. range_*: all four set or all NULL
        -- (docs/api.md "Range comments"). resolved: staged thread decision
        -- (NULL = none; docs/api.md "Thread resolution"), applied on publish.
```

That is the whole schema — no `changes`/`revisions`/`comments`/`reviews`/
`events` tables; all of that is folded state, and the side tables hold only
registration identity.

## The log

An entry is `(chain_id, idx, kind, payload, created_at)`. `idx` is 0-based
and contiguous, so a cursor is just an offset: an agent that consumed
`[0, c)` reconnects the `events` stream with `c` and gets `[c, head)` then
the live tail (docs/api.md "events"). Five kinds:

| kind           | appended by                                   |
| -------------- | --------------------------------------------- |
| `revisions`    | a scan that changed structure (push/rescan)   |
| `review`       | reviewer submits a verdict (`POST …/reviews`) |
| `comment`      | an agent posts a comment (`nit comment`)      |
| `partial`      | `nit push --partial` / `nit ready` flips it   |
| `chain_closed` | a scan finds the chain merged/abandoned       |

Every entry streams to every `events` consumer unfiltered; whether one
_wakes_ a parked `nit wait` is a client decision ("Wake rule").

### Identity within the log

Two kinds of fold-assigned id, opaque and stable across replays:

- **Change and review ids** come from a per-chain counter at append time,
  written into the payload. Replay trusts the stored ids and resumes the
  counter at `max(seen) + 1`; a draft's id is drawn from the same counter,
  so it never collides.
- **Thread ids** are **not stored**. A thread is born when a comment folds
  with no `thread_id`, numbered by creation order as the fold replays — a
  pure function of the log, so every replay assigns the same id to the same
  thread. Later comments (and reviewer drafts) join by carrying the
  `thread_id`. The thread exists only as a fact of the fold.

### Payloads

```jsonc
// revisions — the structural delta of one scan (see "Scan algorithm")
{
  // live changes, in order; change_id is fold-assigned (minted at append
  // time for a new key, reused for an existing one)
  "live": [{"change_key": "I3f2…", "change_id": 10, "position": 0}, …],
  "added": [                                              // changes that got a NEW revision
    {"change_key": "I3f2…", "number": 2, "commit_sha": "…",
     "parent_sha": "…", "message": "full commit message\n…",
     "resets_status": true}                               // false only for a pure rebase
  ]
}
// Orphaned = changes in the fold but absent from `live`; reattached = changes
// in `live` that were orphaned. The fold derives both by diffing `live`.

// review — one reviewer verdict on one change, draining its drafts. Each
// comment opens a new thread (thread_id null, anchor used) or replies to an
// existing one (thread_id set, anchor ignored).
{
  "change_key": "I3f2…",
  "review_id": 5,                      // fold-assigned (stored), like change ids
  "revision": 2,                       // the reviewed revision (post pure-rebase retarget)
  "verdict": "request_changes",        // approve | request_changes | comment
  "message": "cover note",
  "comments": [                        // the drained drafts, in draft order
    {"thread_id": null,                // null = opens a new thread; set = reply to it
     "revision": 2, "file": "src/main.rs", "line": 14, "side": "new",
     "range": null, "line_text": "    let x = …",  // anchor — used only for a new thread
     "body": "…",                      // empty body = resolution-only (adds no comment)
     "resolved": true}                 // staged thread decision; null = none
  ]
}

// comment — one comment an agent posts (`nit comment`): opens a thread or
// continues one. The agent-authored mirror of a single review comment.
{
  "change_key": "I3f2…",
  "thread_id": null,                   // null = open a new thread (anchor below); set = reply
  "revision": 2, "file": "Cargo.toml", "line": 14, "side": "new",
  "range": null, "line_text": "serde = …",         // anchor — used only for a new thread
  "body": "why this dep",
  "resolved": true                     // new thread: born resolved/open; reply: resolve/reopen; null = unchanged
}

// partial — sticky more-commits-coming flag
{"partial": true}

// chain_closed
{"status": "merged"}                   // merged | abandoned
```

## The fold (log → state)

Per change, the state holds its **threads** — each a located, resolvable
conversation: an anchor (revision/file/line/side/range/line_text), a
rolled-up `resolved` flag, and ordered comments (author + body). Replaying
the log in order yields this. Each kind's effect:

- **`revisions`** — per `added`: create the change if its key is new
  (status `pending`), append the revision, set status `pending` when
  `resets_status`. Apply `live`: set each change's `position` and clear its
  orphaned flag; a change absent from `live` becomes **orphaned**
  (`position = null`, threads/reviews kept, status retained); a
  previously-orphaned change present in `live` is reattached.
- **`review`** — apply each comment to the change's threads (below) as
  `reviewer`, tagged with `review_id`; record the review (verdict, message,
  reviewed revision); set status from the verdict (`approve`→approved,
  `request_changes`→changes_requested, `comment`→commented).
- **`comment`** — apply the one comment as `agent`, no `review_id`. Adds no
  review and leaves status untouched — an agent note is not a verdict.
- **`partial`** / **`chain_closed`** — set the partial flag / the
  merged/abandoned status.

**Applying a comment** (shared by `review` and `comment`): with no
`thread_id`, mint the next id and open a thread at the anchor — first
comment = author + body, `resolved` from the comment's decision. With a
`thread_id`, append author + body to that thread (empty body adds no
comment, only the resolution) and apply the `resolved` decision
(true→resolved, false→reopened, null→unchanged). Anchor and birth come from
the first comment; later comments may only move the flag, so a thread ends
at the **last** decision applied.

A change's wire `status` is `orphaned` when flagged, else its retained
status (`pending | approved | changes_requested | commented`); `position`
is null while orphaned.

### Change identity (`change_key`)

The **`Change-Id:` trailer** (gerrit-style, any opaque token) is the
identity, required and canonical: every commit in `base..tip` must carry
its own, or the scan aborts (no entry appended, `last_scan_error`
surfaced) — same for a duplicate trailer, a `fixup!`/`squash!` (squash
locally first), or a merge commit. A change keeps its identity and thread
history across sha changes (rebase, amend, reword); a new trailer is a new
change. A change whose trailer leaves the walk becomes **orphaned**
(lossless — threads and reviews kept, shown collapsed); the trailer
returning reattaches it. Orphans keep transient git states (mid-rebase
resets, dropped-and-restored commits) lossless.

## Scan algorithm (push + throttled on reads, under the chain lock)

A scan reconciles git reality against the fold and **appends one
`revisions` entry iff the structure changed** (so read-scans never bloat
the log):

1. Open repo; resolve `base` and tip. Failure (repo moved, base gone) →
   set `last_scan_error`, keep the fold, append nothing.
   - Branch ref missing on **two consecutive scans ≥ 10s apart** →
     `chain_closed{abandoned}` (the gap protects against mid-rebase
     windows; the timer is in-memory, so a restart resets it — abandonment
     is best-effort, delayed not wrong). The first miss is just the
     `last_scan_error` marker.
   - Merged test: tip is ancestor-or-equal of base **and** every live
     change matches a commit in `fork..base` (_fork_ = `parent_sha` of the
     first live change's latest revision). A change matches by Change-Id
     first, then by diff patch-id; empty diffs match trivially but ≥1 real
     match is required. No live changes → judge the orphans. Match →
     `chain_closed{merged}`.
   - A later scan that finds the branch alive reopens a merged/abandoned
     chain.
2. Walk `base..tip` oldest-first. A merge commit or a root commit in the
   range aborts the scan ("rebase onto the base instead"); so does any
   commit missing its `Change-Id:` or being a `fixup!`/`squash!` (kept fold
   - `last_scan_error`).
3. Diff the walk against the fold: a new key → `added` (number 1); a tip
   sha that differs from the change's latest revision → `added`
   (number + 1), with `resets_status = false` only for a **pure rebase**
   (patch-id-equal **and** message unchanged), else `true` (a reword
   counts — the message is reviewable as `/COMMIT_MSG`). Live ordering →
   `live`; keys gone from the walk drop out (the fold orphans them).
4. Append the `revisions` entry iff step 3 produced anything.

## Diffs

Diffs are never stored. The fold holds each revision's `commit_sha` and
`parent_sha`; the diff endpoint opens the repo and computes
`parent_tree → commit_tree` (or `tree(m) → tree(n)` for an interdiff) with
libgit2 per request. Commit messages render as the synthetic `/COMMIT_MSG`
file (api.md).

## Wake rule

The server streams every entry on `events` unfiltered; the wake rule is a
**client** concern. The default is **wake** — every event ends a parked
`nit wait`: a reviewer verdict, a new `revisions`, `partial`,
`chain_closed`, even the agent's own pushes and comments (skimmed with
`--oneline`). One case is suppressed:

> a `review` with verdict `approve`, **no comments**, that does **not**
> complete the chain (does not reach `approved`).

So a reviewer approving change after change doesn't wake the agent each
time. It is **not dropped**: the client accumulates it and hands it back
with the next waking event (a completing approve reaches `approved` and
wakes; `nit wait` detects completion via `feedback.actionable`). A fresh
`wait`/`log` from an earlier cursor still sees it. With no timeout, a
suppressed approve never surfaces a wait on its own.

## Derived chain state

The fold yields the same actionable state the agent branches on:

```
change (wire):  orphaned  when the orphaned flag is set
                else pending | approved | changes_requested | commented

chain state (derived from the live changes):
  status merged/abandoned                     → merged / abandoned
  any live change changes_requested|commented → agents_turn
  else any live change pending                → waiting_for_review
  else all approved (≥1)                      → agents_turn if partial
                                                (still pushing), else approved
  else (no live changes)                      → agents_turn   (empty chain)
```

The full actionable/feedback contract lives in [api.md](api.md).

## Concurrency (normative)

- One **per-chain async mutex** serializes every appender (scan, review,
  agent comment, partial flip). Under it the batch is fold-validated on a
  throwaway copy, the log rows inserted in one `BEGIN IMMEDIATE`
  transaction, the fold updated, and only then each entry published on the
  chain's broadcast channel. Validate-before-commit keeps the log from
  getting ahead of the fold; publish-after lets a subscriber reconcile the
  channel against its backlog without seeing a half-applied entry.
- An `/events` subscriber arms its subscription **before** reading the
  backlog, then drops any streamed entry the backlog already covers — so
  each entry reaches a live subscriber exactly once. One lagging past the
  channel buffer is dropped with an overflow signal and reconnects at its
  cursor.
- Scans throttle: one that finished < 2s ago is not repeated; reads serve
  the current fold instead of waiting on a running scan.
- A failed scan **never** partially reconciles: no entry appended,
  `last_scan_error` set, the prior fold served. One broken chain must not
  affect the others.

## GC safety

The commit objects review history points at must survive `git gc` and
post-merge reflog expiry. After each scan, on active chains, nit maintains
one ref per revision of every change (orphans included):
`refs/nit/keep/<chain-id>/<change-id>/<revision-number>` → the revision's
commit (its parent, the diff's old side, is reachable through it). Closing
a chain deletes its refs.
