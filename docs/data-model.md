# Data model

Review state is an **append-only event log**. Each chain owns a log of
entries; the chain's entire reviewable state — its changes, revisions,
comment threads, reviews, partial flag, open/closed status — is the
**fold** of that log. Nothing in the log is ever mutated or deleted; a
correction is a new entry. The server holds the fold in memory and
**replays the log on startup** to rebuild it; SQLite stores only the log
(plus three non-history side tables: the repo registry, chain
registration, and reviewer drafts).

git objects stay in the user's repo, pinned where needed (see "GC
safety"). Diffs are never stored — they are computed on demand from the
commit shas an entry carries (see "Diffs").

## Tables

```sql
repos   (id, git_dir, UNIQUE(git_dir))
        -- the repository registry: a canonical git-common-dir → id, the
        -- grouping/index key chains hang off. Stores nothing derivable from
        -- git — no branches, bases, commits, or timestamps; those live in
        -- .git, fetched on demand. git_dir is the repo's identity *and* its
        -- display name; `nit repo move` repoints it after a disk move.

chains  (id, repo_id, branch, base, created_at, UNIQUE(repo_id, branch))
        -- registration identity only, grouped under a repo (repo_id → repos).
        -- The repo's git_dir is the path every git op opens; everything else
        -- about a chain (status, partial, changes, comments…) is folded
        -- from its log, never stored here.

log     (chain_id, idx, kind, payload, created_at,
         PRIMARY KEY (chain_id, idx))
        -- the append-only event log. idx is 0-based and contiguous per chain
        -- (idx 0 is the first entry). payload is kind-specific JSON (below).
        -- head(chain) = number of entries = idx of the next entry to append.

drafts  (id, chain_id, change_key, revision, thread_id, file, line, side,
         range_start_line, range_start_char, range_end_line, range_end_char,
         line_text, body, resolved, created_at, updated_at)
        -- reviewer-private scratch: unpublished comments. Mutable
        -- (PATCH/DELETE) and NOT part of any chain's history — drafts never
        -- enter the log. Publishing a review drains a change's drafts into one
        -- `review` entry and deletes the rows. thread_id (fold-assigned, see
        -- below) is set when the draft replies to an existing thread; NULL
        -- means the draft opens a new thread anchored by file/line/side/range.
        -- side: old | new. range_*: all four set or all NULL, range_end_line =
        -- line (docs/api.md "Range comments"). resolved: the staged
        -- thread-resolution decision (NULL = none; docs/api.md "Thread
        -- resolution"), applied when the draft publishes.
```

That is the whole schema. There are no `changes`, `revisions`,
`comments` (published), `reviews`, or `events` tables — all of that is
folded state. The `repos`/`chains` side tables hold only registration
identity (the grouping above); a chain's reviewable state is its log.

## The log

An entry is `(chain_id, idx, kind, payload, created_at)`. `idx` is
0-based and contiguous, so a cursor is just an offset: an agent that has
consumed entries `[0, c)` reconnects the `events` stream with `c` and
receives `[c, head)` then the live tail (docs/api.md "events"). Five kinds:

| kind           | appended by                                   |
| -------------- | --------------------------------------------- |
| `revisions`    | a scan that changed structure (push/rescan)   |
| `review`       | reviewer submits a verdict (`POST …/reviews`) |
| `comment`      | an agent posts a comment (`nit comment`)      |
| `partial`      | `nit push --partial` / `nit ready` flips it   |
| `chain_closed` | a scan finds the chain merged/abandoned       |

Every entry is streamed to every connected `events` consumer, unfiltered.
Whether an entry should _wake_ a parked `nit wait` is a client decision,
not a property of the log (see "Wake rule").

### Identity within the log

A chain mints two kinds of fold-assigned id, both opaque and stable across
replays:

- **Change and review ids** come from a per-chain counter **at append time**
  and are written into the payload (a `revisions` entry carries each live
  change's id; a `review` its `review_id`). Replay trusts the stored ids and
  resumes the counter at `max(seen) + 1`. A draft's id is drawn from the same
  counter, so it never collides.
- **Thread ids** are **not stored**. A thread is born the first time a comment
  is folded with no `thread_id`, and is numbered by **creation order** as the
  fold replays — a pure function of the log, so every replay assigns the same
  id to the same thread. A later comment joins a thread by carrying its
  `thread_id`, and a reviewer draft references one the same way. Nothing about
  a thread lives in the log or the database beyond the comments that
  constitute it; the thread is entirely a fact of the fold.

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

A chain's state holds, per change, its **threads** — each a located,
resolvable conversation: an anchor (revision/file/line/side/range/line_text),
a rolled-up `resolved` flag, and an ordered list of comments (each an author
and a body). Replaying the log in order yields this state. Each kind's effect:

- **`revisions`** — for each `added`: create the change if its key is new
  (status `pending`), append the revision, and set status `pending` when
  `resets_status`. Apply `live`: set each listed change's `position` and
  clear its orphaned flag; any change absent from `live` becomes
  **orphaned** (`position = null`, threads/reviews kept, underlying
  status retained); a previously-orphaned change present in `live` is
  reattached, its retained status exposed again.
- **`review`** — apply each listed comment to the change's threads (below),
  authored by `reviewer` and tagged with this `review_id`; record the review
  (verdict + message + reviewed revision); set the change's status to the
  verdict (`approve`→approved, `request_changes`→changes_requested,
  `comment`→commented).
- **`comment`** — apply the single comment to the change's threads, authored
  by `agent` with no `review_id`. Adds no review and **leaves the change's
  status untouched** — an agent's note is not a verdict.
- **`partial`** — set the chain's partial flag.
- **`chain_closed`** — set the chain's status (merged/abandoned).

**Applying a comment** (shared by `review` and `comment`): with no
`thread_id`, mint the next thread id and open a thread at the comment's
anchor — its first comment the given author + body, its `resolved` the
comment's decision (`true`→resolved, else open). With a `thread_id`, append
the author + body to that thread (an empty body adds no comment, only its
resolution) and apply any `resolved` decision (`true`→resolved,
`false`→reopened, null→unchanged). A thread's anchor and birth come from its
first comment; later comments only extend the conversation and may move the
flag — so a thread ends at the **last** decision applied to it.

A change's wire `status` is `orphaned` when its orphaned flag is set,
else its retained status (`pending | approved | changes_requested |
commented`). Position is `null` while orphaned.

### Change identity (`change_key`)

The **`Change-Id:` trailer** (gerrit-style, any opaque token) is the
identity, required and canonical: every commit in `base..tip` must carry
its own — a missing trailer, a token shared by two commits, or a
`fixup!`/`squash!` commit (squash locally before pushing) aborts the scan
(no entry appended; `last_scan_error` surfaced), like merge commits.

A change keeps its identity — and its thread history — while its commit
sha changes (rebase, amend, reword); changing a commit's Change-Id makes
it a new change. Commits with a new trailer become new changes; a change
whose trailer leaves the walk becomes **orphaned** (lossless — threads,
reviews kept, UI shows it collapsed); the trailer returning reattaches
it. Orphans are how transient git states (mid-rebase resets,
dropped-and-restored commits) stay lossless.

## Scan algorithm (push + throttled on reads, always under the chain lock)

A scan reconciles the chain's git reality against the current fold and
**appends one `revisions` entry iff the structure changed** (so
throttled read-scans never bloat the log). Steps:

1. Open repo; resolve `base` and branch tip. Failures (repo moved, base
   gone) → set `last_scan_error`, keep the fold, append nothing.
   - Branch ref missing: only after the ref is missing on **two
     consecutive scans ≥ 10s apart** → append `chain_closed{abandoned}`
     (protects against mid-rebase windows). The first observation is the
     branch-missing `last_scan_error` marker. The 10s timer is in-memory
     transient state (not folded from the log), so a server restart resets
     the window — abandonment is best-effort and merely delayed, never
     wrong.
   - Merged test: tip is ancestor-or-equal of base **and** every live
     change matches a commit in `fork..base`, where _fork_ is the
     `parent_sha` of the first live change's latest revision. A change
     matches by **Change-Id trailer first**, then by the patch-id of its
     diff (`parent_sha → commit tree`); empty diffs match trivially but
     at least one real match is required. If no live changes exist, the
     orphans are judged instead. Match → append `chain_closed{merged}`.
   - A later scan that finds the branch alive with commits reopens a
     merged/abandoned chain (its next `revisions` entry rebuilds it).
2. Walk `base..tip` oldest-first. **Any merge commit** aborts the scan
   ("chain contains merge commits — rebase onto the base instead"); so
   does a root commit in the range. Then validate identity: every walked
   commit carries its own `Change-Id:` and is not a `fixup!`/`squash!`
   — violations abort the same way (kept fold + `last_scan_error`).
3. Diff the walk against the fold: new keys → `added` (number 1); a
   change whose tip sha differs from its latest revision's → `added`
   (number + 1) with `resets_status = false` only for a **pure rebase**
   (patch-id-equal **and** commit message unchanged — review submission
   auto-retargets, see api.md), else `true` (a reword counts, the message
   is reviewable as `/COMMIT_MSG`); the post-walk live ordering → `live`;
   keys gone from the walk drop out of `live` (the fold orphans them).
4. If anything in step 3 is non-empty, append the `revisions` entry (it
   then streams to `events` consumers like any other); otherwise append
   nothing.

## Diffs

A diff is never stored. The fold holds each revision's `commit_sha` and
`parent_sha`; the diff endpoint opens the repo and computes
`parent_tree → commit_tree` (or `tree(m) → tree(n)` for an interdiff)
with libgit2 per request. Commit messages (held in the fold) render as
the synthetic `/COMMIT_MSG` file (api.md). Interdiff m→n is
`tree(m) → tree(n)`.

## Wake rule

The server does **not** decide relevance: it streams every entry on
`events` (api.md), unfiltered. The wake rule is a **client** concern —
`nit wait` (or, later, an event-driven UI) reads the stream and decides
which entries should end a parked wait. The default is **wake** — every
event ends the wait, so the agent reacts to a reviewer verdict
(`request_changes` / comment-only / a completing `approve`), a new
`revisions`, `partial`, or `chain_closed`, and even its own pushes and
`comment`s (it skims those with `--oneline`). There is exactly **one**
suppressed case:

> a `review` with verdict `approve`, **no comments**, that does **not**
> complete the chain (does not reach `approved`).

A reviewer working through a chain approving change after change should
not wake the agent each time, so `nit wait` does not return on a
non-completing pure-approve. It is **not dropped**: the client accumulates
it and hands it back with the next event that does wake (an approve that
_completes_ the chain leaves the chain actionable — `approved` — and
wakes). `nit wait` recognises completion via the chain's
`feedback.actionable`; a fresh `nit wait`/`nit log` from a later cursor
still sees the suppressed entry. Because there is no timeout, a suppressed
approve never surfaces a parked wait on its own.

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

- One **per-chain async mutex** serializes every scan, review submission,
  agent comment, and partial flip — i.e. every appender. Under that lock
  the batch is fold-validated on a throwaway copy, the log rows are inserted
  in one `BEGIN IMMEDIATE` transaction, the live fold is updated, and only
  then is each entry published on the chain's broadcast channel (the feed
  `/events` subscribers read). Validating before the commit keeps the log
  from ever getting ahead of the fold; publishing after it lets a subscriber
  reconcile the channel against its log backlog without seeing a
  half-applied entry.
- An `/events` subscriber arms its broadcast subscription **before** reading
  the log backlog, then drops any streamed entry whose `idx` the backlog
  already covers. So each appended entry reaches a live subscriber exactly
  once — nothing slips through the gap between subscribe and read, and the
  overlap is de-duplicated. A subscriber that lags past the channel buffer
  is dropped with an overflow signal and reconnects at its cursor.
- Scans are throttled: a scan that finished < 2s ago is not repeated;
  reads serve the current fold instead of waiting on a running scan.
- A failed scan **never** partially reconciles: no entry is appended,
  `last_scan_error` is set, the prior fold stays served. One broken chain
  must not affect the others.

## GC safety

The commit objects review history points at must survive `git gc` and
post-merge reflog expiry. After each scan nit maintains one ref per
revision of every change — orphans included — on active chains:
`refs/nit/keep/<chain-id>/<change-id>/<revision-number>` → the
revision's commit (its parent, the diff's old side, is reachable through
it). Refs for merged/abandoned chains are deleted by the scan that closes
them.
