# Data model

The unit of state is the **change** (a `Change-Id`, scoped to a repo). A
change owns an **append-only log** whose **fold** is its entire reviewable
state — revisions, comment threads, reviews, partial flag, lifecycle.
Nothing is mutated or deleted; a correction is a new entry. The server holds
each change's fold in memory, rebuilt by replaying its log on startup.

A **chain is never stored** — not in SQLite, not in memory. It is derived at
read time by walking a tip commit's `parent_sha` back to the repo's canonical
base branch through each revision's recorded parent (gerrit relation chains).
Order is a read-time walk, never stored state.

SQLite stores only the per-change logs plus four side tables (the repo
registry, the change-identity registry, reviewer comment drafts, and reviewer
decision drafts). The lone piece of derived state stored is a denormalized
`status` on each change row — a cache of the fold's current status (below) so a
query can filter changes without replaying logs; the log stays authoritative.
git objects stay in the user's repo, pinned against `git gc` ("Keep refs");
diffs are computed on demand from the commit shas a revision carries, never
stored ("Diffs").

Live followers (`nit wait`, `nit log --follow`) watch a set of changes over
one websocket (docs/api.md "Events"); which entries **wake** a parked wait is
a client decision ("Wake rule" below). The web polls the same folds.

## Tables

```sql
repos   (id, git_dir, base_ref, base_head, UNIQUE(git_dir))
        -- the registry. git_dir is the canonical git-common-dir — the repo's
        -- identity *and* display name; `nit repo move` repoints it. base_ref
        -- is the repo's one canonical base ref — any git ref (a local branch,
        -- `origin/main`, a tag), set at `nit repo create`; mergedness always
        -- tracks it (there is no land-anywhere). base_head is the merge timer's
        -- baseline — the base ref's HEAD it last reconciled against ("Lifecycle
        -- timer"), the one stored value that is observation state,
        -- not git-derivable. Stores nothing else derivable from git (no commits,
        -- timestamps — those live in .git).

changes (id, repo_id, change_key, status, created_at, UNIQUE(repo_id, change_key))
        -- the identity registry: a (repo, Change-Id) → a stable rowid `id` that
        -- everything carries. Never reused, so a cursor key is valid for the
        -- life of the repo. Reviewable state is the fold of this change's log,
        -- not stored here — except `status`, a denormalized cache of the fold's
        -- current status (the displayed status at the latest revision) so a
        -- query can filter/scan changes without replaying logs. The log stays
        -- authoritative: `status` is re-stamped in each append's transaction and
        -- reconciled against the fold on startup (rewritten only when it has
        -- drifted); NULL only before a change's first append (a torn push).

log     (seq, change_id, idx, kind, payload, created_at,
         PRIMARY KEY (seq AUTOINCREMENT), UNIQUE(change_id, idx))
        -- the append-only log, keyed on the change. Every entry carries TWO
        -- coordinates: a per-change `idx` (0-based, contiguous) and a global
        -- `seq` (the rowid, monotone across the whole repo). payload is
        -- kind-specific JSON (below). head(change) = entry count = idx of the
        -- next entry to append.

draft_comments (id, change_id, revision, thread_id, file, line, side,
         range_start_line, range_start_char, range_end_line, range_end_char,
         line_text, body, resolved, created_at, updated_at)
        -- reviewer-private unpublished comments. Mutable (PATCH/DELETE),
        -- never in the log: publishing a review drains a change's drafts into
        -- one `review` entry and deletes the rows. thread_id set = reply to that
        -- thread; NULL = opens a new thread (file/line/side/range anchor). side:
        -- old | new. range_*: all four set or all NULL (docs/api.md "Range
        -- comments"). resolved: staged thread decision (NULL = none; docs/api.md
        -- "Thread resolution"), applied on publish.

draft_reviews (change_id PRIMARY KEY, decision, message)
        -- reviewer-private staged DECISION, one mutable row per change (PUT
        -- overwrites, DELETE clears), never in the log — the decision analogue of
        -- a comment draft. decision is a `Decision` (approve | request_changes |
        -- comment | abandon | reopen); message is the cover note / abandon reason.
        -- No revision column: a decision is change-wide, and the chain path
        -- supplies the revision at publish time (docs/api.md "Reviewer
        -- decisions"). Drained + deleted in the same per-change transaction as
        -- the publish it produces.
```

That is the whole schema — **no `chains` table, no `revisions` table, no
`comments`/`reviews`/`events` tables**. Revision data lives in `revision`-kind
log entries; threads, reviews, status, lifecycle are all folded state; the
side tables hold only registration identity and reviewer scratch — save the
denormalized `changes.status` cache (above), a query index the fold owns.

### The two coordinates

`idx` orders one change in isolation — what a change's own cursor advances
(consumed `[0, c)` ⇒ resume at `c`). `seq` total-orders entries drawn from
_different_ changes: the aggregated chain log (docs/api.md "Chains") sorts by
it. SQLite mints both in one append — `idx = MAX(idx)+1` for the change under
its append lock, `seq` the autoincrement rowid.

## The log

An entry is `(seq, change_id, idx, kind, payload, created_at)`. Five kinds:

| kind        | appended by                                              |
| ----------- | -------------------------------------------------------- |
| `revision`  | a push observes a new commit-sha for this change         |
| `review`    | a reviewer verdict, draining the change's drafts         |
| `comment`   | an agent comment (`nit comment`)                         |
| `lifecycle` | the merge timer; `nit abandon` / `nit reopen`            |
| `partial`   | `nit ready` (or a push) re-stamps the tip's partial flag |

`push` is the only writer of `revision`; the background timer is the only
writer of `merged` `lifecycle` entries; `abandoned`/`reopened` are written on
request (the abandon/reopen actions, "Lifecycle timer").

### Identity within the log

Three kinds of id, all opaque and stable across replays:

- **`change_key`** — the `Change-Id:` trailer verbatim. The change's `id` is
  the `changes` rowid the `UNIQUE(repo_id, change_key)` upsert assigns.
- **Review ids** — minted from a process-global counter at append time and
  written into the `review` payload. Replay trusts the stored id and resumes
  the counter at `max(seen) + 1`; a draft's id is drawn from the same counter,
  so it never collides.
- **Revision numbers** are **not stored** — they are minted **in the fold** by
  creation order (0-based, assigned to each `revision` entry as it folds), a
  pure function of the log, so every replay assigns the same number.
- **Thread ids** are minted in the fold, in one place. The fold takes an entry
  by value and fills a new-thread comment's `thread_id` from the change's
  `next_thread_id` before applying it, returning the entry with the id written
  into its payload — so the append (holding the projection write lock, so the
  counter can't race) stores and broadcasts that one value, and no reader
  re-derives it. `next_thread_id` is the single field minting touches: a new id
  bumps it once; a comment that already names a thread (a reply, or a replayed
  entry whose id is set) only keeps it ahead — never a double count. A comment
  naming a not-yet-seen thread opens it.

### Payloads

```jsonc
// revision — one new commit-sha observed for this change. The revision
// `number` is NOT carried; the fold mints it (0-based, by append order).
{
  "commit_sha": "…",
  "parent_sha": "…",          // the previous walked commit, or the fork for the first
  "base_sha": "…",            // the walk's fork point on the canonical branch
  "message": "full commit message\n…",
  "partial": true,            // this push's partial flag
  "resets_status": true       // false only for a pure rebase (patch-id-equal AND
                              // message unchanged): the new revision then inherits
                              // the prior revision's status instead of pending
}

// review — one reviewer verdict on one change at one revision, draining its
// drafts. Each comment opens a new thread (thread_id null, anchor used) or
// replies to an existing one (thread_id set, anchor ignored).
{
  "review_id": 5,                      // fold-assigned (stored)
  "revision": 2,                       // the reviewed patchset (some live tip pins it)
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
  "thread_id": null,                   // null = open a new thread (anchor below); set = reply
  "revision": 2, "file": "Cargo.toml", "line": 14, "side": "new",
  "range": null, "line_text": "serde = …",         // anchor — used only for a new thread
  "body": "why this dep",
  "resolved": true                     // new thread: born resolved/open; reply: resolve/reopen; null = unchanged
}

// lifecycle — the merge timer (merged), and `nit abandon` / `nit reopen`
{"action": "merged", "revision": 2}    // merged | abandoned | reopened;
                                       // `revision` set only for merged (which patchset landed);
                                       // `message` optional reason on abandoned

// partial — re-stamp the tip change's latest-revision partial flag
{"partial": true}
```

## The fold (log → state)

Per change, the fold holds its **revisions** (each with its shas, message,
partial flag and `resets_status`), its **threads** (each a located, resolvable
conversation: an anchor — revision/file/line/side/range/line_text — a rolled-up
`resolved` flag, and ordered comments — each a body + `review_id`), its **reviews**, and
its **lifecycle** (active / merged{revision} / abandoned). Replaying the log in
order yields this. Each kind's effect:

- **`revision`** — mint the next revision number (0-based) and push a revision
  with the payload's shas/message/partial/`resets_status`.
- **`review`** — record the review (id, verdict, message, reviewed revision),
  then apply each comment to the change's threads (below), tagged with the
  review's `review_id`.
- **`comment`** — apply the one comment with no `review_id` — which is what
  marks it agent-authored. Adds no review and leaves status untouched — an
  agent note is not a verdict.
- **`partial`** — set the latest revision's partial flag.
- **`lifecycle`** — set the change's lifecycle: `merged{revision}`,
  `abandoned`, or `reopened` (back to active).

**Applying a comment** (shared by `review` and `comment`): with no `thread_id`,
mint the next id and open a thread at the comment's anchor — first comment =
body + `review_id`, `resolved` from the comment's decision (a new thread needs a
non-empty body; an empty one is dropped, never minting an id). With a
`thread_id`, append the body + `review_id` to that thread (empty body adds no comment,
only the resolution) and apply the `resolved` decision (true→resolved,
false→reopened, null→unchanged). The anchor and birth come from the first
comment; later comments may only move the flag, so a thread ends at the **last**
decision applied.

### Per-change, per-revision status

A change's authoritative status is per `(change, revision)` — its **displayed
status**: the lifecycle overlay over the verdict-derived review status of the
patchset a path pins. (The `changes.status` column denormalizes one coarse
scalar from this — the displayed status at the **latest** revision — as a query
index; chain derivation and every status read still compute the per-revision
value below, never consult the column.)

```
status:  pending | approved | changes_requested | commented | merged | abandoned
```

- **Review status at a revision** = the verdict of the latest review whose
  `revision` is that patchset (`approve`→approved, `request_changes`→
  changes_requested, `comment`→commented). No review there: a **pure-rebase**
  revision (`resets_status == false`) carries the **prior** revision's status
  forward; otherwise `pending`.
- **Lifecycle overlay**: `abandoned` is terminal **change-wide** (every
  revision). `merged` is terminal only for the **landed** patchset — a path
  pinning a non-landed revision of a merged change shows that member live with
  its own review status, not `merged`.

Two tips pinning two patchsets carry two independent verdicts — an approve in
one chain never overwrites a request_changes in another, because each is scoped
to its `(change, revision)` pair.

### Reviewer decisions (staged, then batch-submitted)

A reviewer's **decision** on a change is reviewer-private scratch, exactly like
a comment draft: it lives in `draft_reviews` (one mutable row per change), never
in the log, and only becomes a fact when published. A `Decision` is a verdict
(`approve`/`request_changes`/`comment`) **or** a lifecycle action
(`abandon`/`reopen`) — abandonment is a staged decision, not a separate button.

The reviewer stages decisions across a chain's members, then **batch submits the
chain** (docs/api.md "Chains" → submit): per member with a staged decision, in
that member's own per-change transaction (atomic per change, never across the
chain — like push), nit publishes the decision **at the revision this chain's
path pins on the member**. The decision row stores no revision, so the
B-in-two-chains member (one change on two chains at two patchsets) publishes at
rev0 from one chain and rev1 from the other — the path is the authority, never a
stored coordinate. A verdict drains the change's comment drafts into one
`review` entry (so a verdict and its comments are inseparable, as before);
`abandon`/`reopen` write a `lifecycle` entry and still drain any comment drafts
into a `comment` review in the same transaction, so staged comments are never
stranded. Publishing deletes the member's `draft_reviews` row, so re-submitting
a batch torn by a transient `SQLITE_BUSY` finishes the rest without
double-publishing. A staged decision illegal for the change's current lifecycle
(a verdict on a terminal change, a `reopen` on a live one) is reported back and
left staged. Batch submit is the only path a reviewer verdict reaches the log —
there is no immediate single-change submit.

### Change identity (`change_key`)

The **`Change-Id:` trailer** (gerrit-style, any opaque token) is the identity,
required and canonical: every commit in `fork..tip` must carry its own, or the
push is rejected whole (`400`, nothing recorded). A change keeps its `id` and
its thread history across sha changes (rebase, amend, reword); a new trailer is
a new change. The same trailer reached by two pushes on different parents is
**one change with two patchsets** ("The B-in-two-chains example"), not a
conflict. A change no tip reaches is simply off every path — its log and threads
retained and pinned, reachable by id (there is no orphaned/position machinery;
order is the SHA-walk).

## Chain derivation

A chain is a pure read-time function of the repo's per-change folds
(`crate::chain::RepoView`). The view owns a snapshot of every change plus a
`commit_sha → (change_id, revision number)` **index** built from their
revisions; it holds no locks and touches no git.

- **Tips** are leaves over the latest revisions: each **non-terminal** change's
  latest-revision `commit_sha` that **no** revision (of any change) records as a
  `parent_sha`. A superseded patchset is never a tip; a terminal change is not a
  tip but can still be an interior member of a live tip's path. (`all_tips`
  drops the non-terminal filter — the `?status=all` view that still surfaces
  recently merged/abandoned chains.)
- **`path_from_tip(sha)`** walks back to the canonical branch, oldest-first:
  resolve `sha` through the index to `(change, revision)`, push it, then follow
  that revision's `parent_sha` — stopping at the branch (the recorded fork
  `parent_sha == base_sha`, or the first parent that has since **merged**) or an
  unresolved parent. Stopping at a merged parent keeps a partially-landed stack
  deriving to its open members alone: as members land, the branch advances past
  the recorded `base_sha` and the walk follows it. The walk is **total**: an
  unresolved parent (below the merge-base, or a torn push) truncates the path,
  never errors; a cycle guard rides out bad data. Each member is pinned to the
  patchset the tip walked through.

### The B-in-two-chains example

Two pushes in one repo, canonical `main` at merge-base `m`:

- push 1: `m → A → B → C` (Change-Ids `Ia, Ib, Ic`)
- push 2: `m → D → B′ → E` (`Id, Ib, Ie`, B re-parented onto D)

`B` is one change with two patchsets: rev0 `parent=A`, rev1 `parent=D`. Two
tips, two chains: the C-chain walks B at rev0, the E-chain walks B at rev1.
Threads and reviews on B are **shared** (they belong to the change) and each is
anchored to the revision it was written against; `?revision` selects which
patchset — and which chain context — you view (rev0 ⇒ the C-chain, rev1 ⇒ the
E-chain), because each revision records the parent that places B in exactly one
chain.

### Derived chain state

`derive_state` folds the members' displayed status (each at its pinned
revision) plus the tip's partial flag into the actionable state the agent
branches on:

**Abandonment is derivation-inert**: an `abandoned` member is dropped from the
fold before the rollup (there is no chain-level abandoned state) — it shows as
`abandoned` on its own path entry, and the agent decides what to do with it.

```
every non-abandoned member merged (at its pinned revision) → merged   (off the main page)
else any member changes_requested|commented      → agents_turn
else any member pending                          → waiting_for_review
else all approved (≥1) and tip partial           → agents_turn   (still pushing)
else all approved (≥1)                           → approved
else (no live members)                           → agents_turn   (empty/all-abandoned tip)
```

A chain is **partial** iff its **tip** change's latest revision is partial —
the tip is the work frontier, so the most recent push's intent governs whether
the chain may merge; a shared interior change carrying a stale partial from a
sibling push does not hold an unrelated chain partial. A chain drops off the
main page iff **every** member is terminal — any one live member keeps a
partially-landed stack visible. The full actionable/feedback contract lives in
[api.md](api.md).

## Push

`push` is the **only writer of revisions**. There is no chain entity, so no
birth decision and no cross-chain routing — every commit is an independent
upsert keyed by its `Change-Id`.

1. Look up the repo by its `git_dir`; an unregistered repo is a `404`
   (`nit repo create` first). The canonical `base` is the repo's stored
   `base_ref` (set at create — push neither takes nor configures a base);
   `base` and `tip` failing to resolve is a `400`.
2. `fork = merge-base(base, tip)`; walk `fork..tip` oldest-first
   (`gitscan::walk_push`). The walk is **all-or-nothing** — a `400` rejects the
   whole push on any structural fault: a merge or root commit, a commit missing
   its `Change-Id`, a duplicate trailer within the walk, a `fixup!`/`squash!`
   subject, or a commit-sha already recorded under a different change. A
   half-valid walk would record a chain shape the reviewer can't trust; nothing
   is recorded, the agent fixes locally and re-pushes.
3. Pre-flight each change: upsert it (`INSERT … ON CONFLICT DO NOTHING`, keyed
   by `(repo_id, change_key)` — idempotent and self-serializing) and **reject
   `409`** any whose content moved while it is **abandoned** — an abandoned
   change must be `nit reopen`'d first, so a stray re-push never silently
   resurrects it.
4. Per commit, oldest-first, with `parent_sha` = the previous walked commit (or
   `fork` for the first): **append a `revision` entry iff the commit-sha moved**
   (differs from the change's latest revision, or there is none). `resets_status`
   is `false` only for a pure rebase (`gitscan::pure_rebase`: patch-id-equal
   **and** message unchanged — a reword resets, it is reviewable as
   `/COMMIT_MSG`). A keep ref is ensured for each new revision.

A push that walks to nothing (`tip` ancestor-or-equal of `base`) is a **409**:
the tip is already merged into the base (or is the base itself), so there is
nothing to review — a stray push of a landed commit is a visible error, not a
silent no-op. **Idempotency**: re-applying the same `(change_id, idx)` is a
no-op at the storage layer, so a crash-retry is safe. `partial` is sticky:
present, it re-stamps the tip's latest revision (a push where nothing moved but
`partial` flips is exactly `nit ready`); absent, the tip inherits its prior
revision's flag.

A push touching N changes commits them in **N per-change transactions**,
oldest-first — **not atomic across changes**. A crash or concurrent reader
mid-push can see some changes recorded and others not; this is made safe by
construction, because `path_from_tip` truncates on an unresolved `parent_sha`
(a torn push renders a partial chain, never an error).

## Lifecycle timer

A background sweep (`run_lifecycle_timer`) is the **only writer** of `merged`
`lifecycle` entries — a push cannot observe the base advancing. It runs every
`NIT_TIMER_INTERVAL_MS` (default 5s) and is **edge-triggered on the canonical
branch**: each repo records the branch HEAD it last reconciled against
(`repos.base_head`, persisted), and a sweep does work only when that HEAD has
moved. A repo whose branch is idle costs one ref resolution and nothing else —
no per-change walk, no diffs.

When the branch has moved, `detect_landings` walks only the **new** commits
`base_head..HEAD` (one walk for the whole repo) and, for each commit carrying a
non-terminal change's `Change-Id` whose patch-id equals that change's latest
revision, appends `lifecycle{merged, revision}` — which patchset landed. A key
present with a _different_ patch-id is "landed earlier, since amended": the
change stays open. An empty diff never counts. Then the sweep records `HEAD` as
the new baseline. Prefix merge falls out for free: in one delta, landing A and B
marks them merged while C — not in the delta — stays live, no chain-level gate.

The baseline is seeded at `nit repo create` to the branch's then-HEAD, so the
first landing after registration shows up in a delta. Persisting it means a
restart resumes from the last reconciled HEAD and still catches landings that
happened while nit was down; a baseline that no longer resolves (a rewritten
branch) is re-adopted with nothing detected that sweep. **Detection is by
`Change-Id` only** — a landing that _stripped_ its trailer is not detected. nit's
own approve action preserves the trailer through rebase + fast-forward, and
chasing keyless landings is what would force an unbounded per-change diff every
sweep; a missed keyless landing is recoverable by re-push or manual abandon.

The sweep skips already-terminal changes (merged or abandoned) — there is no
point merge-checking a dead change. It never abandons: **abandonment is an
explicit action**, not a ref-reachability observation. "Off every branch" is
not terminal — a detached-HEAD or post-rebase-orphan change stays live as its
own chain until a human (or the owning agent) abandons it.

**Abandon and reopen are explicit.** `nit abandon` appends
`lifecycle{abandoned}` (a reviewer/agent judgment, optionally with a reason
message); the change goes terminal but stays a member/tip of its chains and
does not roll up to a chain state. `nit reopen` appends `lifecycle{reopened}`,
clearing `abandoned` back to the retained verdict status; the agent may then
push a new revision (which folds it to `pending`). A push to an abandoned
change is a 409 ("reopen first"). Both are durable log facts — there is no
auto-correction.

## Diffs

Diffs are never stored. The fold holds each revision's `commit_sha`,
`parent_sha` and `base_sha`; the diff endpoint opens the repo and computes
`parent_tree → commit_tree` (or `tree(m) → tree(n)` for an interdiff) with
libgit2 per request. Commit messages render as the synthetic `/COMMIT_MSG`
file, and an interdiff across re-parented revisions is drift-processed
(docs/api.md "Rebase-aware interdiffs").

## Concurrency (normative)

The unit of contention is **one change's log**. A chain owns no log and no
lock. Each loaded change holds `proj: StdRwLock<ChangeProj>` (the fold) and a
sync **append lock** (`StdMutex`) serializing its appenders — no async mutex,
no per-chain lock.

- **Append** (`append_to_change`) runs off the async runtime on a pooled
  connection under the change's append lock. It reads the committed `head` for the change's `idx`,
  **validates the batch by folding it onto a throwaway probe copy** (a payload
  that won't fold errors out with nothing written), then commits the log rows
  under `BEGIN IMMEDIATE` (minting each `seq`), and only then folds the entries
  into the live projection (infallible — already validated). A `review`'s draft
  drain shares the same transaction, so either both land or neither does.
  Validate-before-commit keeps the log from getting ahead of the fold; the held
  guard spans only the in-memory fold (microseconds), never the blocking commit,
  so a reader never stalls on a writer's I/O.
- **Chain-view assembly** clones each change out from under its lock into an
  owned `RepoView` snapshot, then walks it lock-free — never holding two change
  guards at once, never blocking an append. It can observe B at rev1 while a
  concurrent push appends B's rev2, which is correct (it reflects the tip that
  was pushed).
- **Cross-change appends never contend**: two changes hold two different guards
  and two disjoint transactions. No appender ever holds two change guards at
  once — the invariant that keeps a multi-change push deadlock-free. SQLite is
  the single writer (WAL, `busy_timeout`); a `SQLITE_BUSY` from cross-change
  write contention surfaces as a retryable 503, not a broken database.
- **Creating a change** is the `UNIQUE(repo_id, change_key)` upsert: two pushes
  first-seeing the same key race one SQLite-serialized statement, the loser is a
  no-op, both read the same `change_id`. No read-decide-insert, no owner routing,
  no per-repo lock.
- **Live followers** subscribe to per-change broadcast channels. Each append
  publishes its tagged entry **after** the durable commit and fold; the
  websocket joins the subscribed changes' channels in a `tokio-stream`
  `StreamMap` (dynamic membership), arming each before replaying the change's
  backlog and watermark-deduping the arm/read overlap (docs/api.md "Events").
  Publish is non-blocking, so a slow follower never stalls an appender.

## Keep refs

The commit objects review history points at must survive `git gc` and post-merge
reflog expiry. After each push (and replay), nit maintains one ref per revision
of a change, **keyed on the change** (a chain is not stored):

```
refs/nit/keep/<change-id>/<revision-number> → the revision's commit
```

The revision's parent (the diff's old side) is reachable through it.
`ensure_keep_ref` is idempotent. **GC/deletion is deferred in this cut** — refs
accumulate (fail-safe: pinning more than necessary never drops an object the
SHA-walk, a vs-parent diff, or the timer's `base_sha..canonical` walk needs).

## Wake rule

The server streams every tagged entry unfiltered; a parked `nit wait` wakes on
**every** new entry past its cursor. A reviewer action reaches the agent the
moment it lands — no client-side filtering (the agent's own pushes sit behind
the cursor it passes back, so they never wake it). The return hands back the
whole gap since the cursor plus the derived chain-state (`feedback`).

`nit log --follow --reviewer-only` mutes the agent's own entries
(`revision`/`comment`/`partial`) and the automatic `merged` lifecycle (the
timer's, not the reviewer's) client-side, relaying only reviewer activity —
verdicts plus the `abandoned`/`reopened` decisions.
**Deferred:** muting a sibling-chain note on a revision the follower's path
does not pin — the first cut wakes and lets the agent triage.
