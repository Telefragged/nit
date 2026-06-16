# Change-centric model

The design for nit's change-centric rewrite, and the unit under review.
Non-obvious calls are raised as review threads on the lines they touch;
unresolved forks are collected under "Open questions".

A **change** is the primary entity. It is identified by its `Change-Id:`
trailer scoped to a repo, and it owns an append-only log whose fold is its
entire reviewable state — revisions, threads, reviews, partial flag,
lifecycle. A **chain** is never stored — not in SQLite, not in memory. It is
computed on demand by walking a tip commit back to its merge-point on the
repo's **canonical branch**, through each revision's recorded parent
commit-sha, producing an ordered path of changes (gerrit relation chains).
The UI and CLI reach chains through a few helper endpoints; the log is keyed
on the change.

The same Change-Id reached by two pushes on different parents is one change
with two patchsets, surfaced as two chains. Everything that follows is built
on that single inversion: order is a read-time walk, never stored state; the
contention unit is one change's log; review verdicts are scoped to the
patchset a path pins. Mergedness is tracked against **one configured
canonical branch per repo** — there is no land-anywhere.

## Entities and schema

```sql
repo (
  id          INTEGER PRIMARY KEY,
  git_dir     TEXT NOT NULL UNIQUE,    -- canonical git-common-dir; identity + name
  base_branch TEXT NOT NULL            -- the one canonical branch; mergedness tracks it
)

change (
  id         INTEGER PRIMARY KEY,     -- rowid; the identity everything carries
  repo_id    INTEGER NOT NULL REFERENCES repo(id),
  change_key TEXT    NOT NULL,        -- the Change-Id trailer, verbatim
  created_at TEXT    NOT NULL,
  UNIQUE (repo_id, change_key)
)

log (
  seq        INTEGER PRIMARY KEY AUTOINCREMENT,  -- globally unique, monotone: the cross-change order
  change_id  INTEGER NOT NULL REFERENCES change(id),
  idx        INTEGER NOT NULL,        -- 0-based, contiguous per change
  kind       TEXT    NOT NULL,        -- revision | review | comment | lifecycle | partial
  payload    TEXT    NOT NULL,
  created_at TEXT    NOT NULL,
  UNIQUE (change_id, idx)
)

drafts ( … )                          -- unchanged; reviewer-private, never in the log
```

`change_id` is the rowid of the `UNIQUE(repo_id, change_key)` row, assigned
by `INSERT OR IGNORE … RETURNING id`. It is stable and never reused, so a
cursor key is valid for the life of the repo even across a change leaving
and rejoining every chain.

Every log entry carries **two coordinates**: a per-change `idx` (0-based,
contiguous — what a change's own cursor advances), and a global `seq`
(monotone across the whole repo, the `log` rowid). `seq` total-orders
entries drawn from _different_ changes: the aggregated chain log sorts by it,
and a follower resubscribes from a known `seq`. `idx` orders one change in
isolation; `seq` orders the union of changes a chain spans. SQLite mints both
in one append.

Revision data lives only in the change logs' `revision` entries; the schema
is these four tables. The SHA-walk resolves a commit through an **in-memory
`commit_sha → (change, revision)` index** — a pure function of the fold,
rebuilt from every change's revisions on replay and extended by the push
handler. It is not persisted: nothing in SQLite maps a commit to a change.

`parent_sha` and `base_sha` live on the revision, not the change: the same
change reached by two pushes sits on two parents over two merge-bases of the
one canonical branch.

### The log kinds

| kind        | appended by                                              |
| ----------- | -------------------------------------------------------- |
| `revision`  | a push observes a new commit-sha for this change         |
| `review`    | a reviewer verdict, draining drafts (against a revision) |
| `comment`   | an agent comment                                         |
| `lifecycle` | the merge/abandon/reopen timer and the explicit reopen   |
| `partial`   | `nit ready` re-stamps the tip change's partial flag      |

A `lifecycle` `merged` entry carries `{revision}` — which patchset landed
(into the canonical branch, the only base there is).

### Chain derivation

A chain is not stored. Given a tip commit-sha `T` in repo `R`:

```
chain_from_tip(R, T):
    path = [], sha = T, seen = {}
    loop:
        rev = index[(R, sha)]      -- the in-memory commit_sha → (change, revision) map
        if rev is None:            break    -- sha is below the merge-base, in base
        if sha in seen:            break    -- cycle guard against bad data
        seen.add(sha)
        path.push((rev.change_id, rev.number))
        if rev.parent_sha == rev.base_sha:  break   -- the fork point on canonical
        sha = rev.parent_sha
    return reverse(path)           -- oldest-first, base → tip
```

Each step resolves one commit-sha through the in-memory index to the
`(change_id, number)` that recorded it, reading the rest from that change's
fold. The path pins each member to the patchset that tip walked through, so
viewing one tip shows each change as it stood in that push — never a newer
revision stacked elsewhere. The walk is **total**: an unresolved
`parent_sha` truncates to a partial chain, never errors, so a reader is safe
against a torn push (below).

### Tips

A **tip** is a commit recorded at push time. When a push resolves a ref, nit
walks the pushed commit's parents back to the merge-point on the canonical
branch (the same walk above); the pushed commit is then a tip in nit. Tips
are held in an **in-memory tip-commit set**, kept **purely for performance
and to drive the chains page** — never to derive any state from. A restart
rebuilds it from the keep refs (every keep'd tip is a candidate); losing it
costs a recompute, never correctness.

Branch refs are **incidental state**: a ref does not decide liveness, it only
_names_ a tip when one exists. Names are resolved at **query time** from the
tip commit, best-effort, in a fixed order: a branch ref (preferring one that
`git branch --contains <tip>` keeps stable as the agent advances the
frontier), else a tag, else the commit subject. A push that advances the tip
keeps the same name; deleting a branch only drops a name, not the tip.

A push whose tip commit is **already registered** at that exact sha under its
Change-Id — i.e. nothing moved — is **rejected**, not silently accepted: the
agent is told its push recorded nothing rather than believing it landed work
(see Push → Idempotency).

### On-demand chain endpoints

Chains are computed when asked for, from the tip-commit set and the SHA-walk;
no chain is stored. Three helpers give clients everything they render:

- `GET /api/chains?repo={id}` → one entry per known **tip commit**: its
  derived path (the changes, oldest-first) and a best-effort **name**
  resolved at query time. Drives the chains/dashboard page. The tip set is
  the only in-memory chain state, and only a cache.
- `GET /api/chains/{change_id}` → the changes related to that change's tip
  commit — the derived path through it, each member pinned to the patchset
  the walk selects.
- `GET /api/chains/{change_id}/log` → the **aggregated** log: gather every
  change on the path, read all their log entries, and return them merged and
  **sorted by global `seq`**. One timeline for the whole chain, assembled
  from per-change logs at read time.

### The B-in-two-chains example

Two pushes in repo `R`, canonical `main` at merge-base `m`:

- push 1: `m → A → B → C`, Change-Ids `Ia, Ib, Ic`.
- push 2: `m → D → B′ → E`, Change-Ids `Id, Ib, Ie` (B re-parented onto D).

`B` is one change with two patchsets: rev0 `commit=B parent=A base=m`, rev1
`commit=B′ parent=D base=m`. Two tips, two chains:

- `chain_from_tip(C)`: C(rev0) → B(**rev0**, parent A) → A(rev0) → base.
- `chain_from_tip(E)`: E(rev0) → B(**rev1**, parent D) → D(rev0) → base.

Threads and reviews on B are shared — they belong to the change — and each
is anchored to the revision it was written against. The `?revision` param
selects which patchset of B you view, which _is_ the choice of chain context
(rev0 ⇒ the C-chain, rev1 ⇒ the E-chain), because each revision records the
parent that places B in exactly one chain.

### What is gone

- The `chains` table — a chain is derived from a tip, on demand.
- The `revision` table — revision data lives in the log's `revision` entries;
  the SHA-walk index is in-memory, rebuilt from the fold on replay. The
  one-commit-one-change guard (the old `UNIQUE(repo_id, commit_sha)`) is now an
  in-memory check the push handler makes against the index — and unreachable
  through valid git, where a commit-sha already carries its Change-Id.
- Per-chain `log (chain_id, idx, …)` — replaced by `log (seq, change_id, idx,
…)`.
- Stored liveness of any kind — no `chains` rows, no tip markers, no `tip`
  log kind. Liveness is the push-time tip-commit set, in memory, perf-only.
- The `position`/`orphaned` fold machinery — order is the SHA-walk; a change
  no tip reaches is simply off every path, its log and threads retained and
  pinned, reachable by id.
- Chain-birth routing (the 0/1/2-owner decision) — `INSERT OR IGNORE` per
  change.

## Push

`push` is the only writer of revisions. It walks the commits between the
canonical branch and a tip and appends a `revision` entry to each change
whose content moved. There is no chain entity, so no birth decision and no
cross-chain routing — every commit is an independent upsert keyed by its
`Change-Id`.

```
POST /api/push
req:  {"git_dir": "/abs/.git", "tip": "feat/x", "base": "main", "partial": true}
resp: PushResult
```

`git_dir` is the canonical git-common-dir, canonicalized server-side; its
`repo` row is upserted. `base` configures the repo's **canonical branch**: it
is recorded on the repo's first push and must match the stored `base_branch`
on every push after, else `400` — a repo has exactly one canonical branch.
`tip` is any ref or rev, resolved to a commit at push time; git is the source
of truth for branch position, nit stores no branch sha.

### Resolve and walk

1. Resolve the canonical `base` and `tip`. Either failing to resolve is a
   `400` — push names what to walk, so an unresolvable endpoint is a bad
   request.
2. `fork = merge-base(base, tip)`. Walk `fork..tip` oldest-first
   (topological). The walk is the chain the reviewer will see.
3. The whole push is rejected `400` on any structural fault: a merge or root
   commit in the walk, a commit missing its `Change-Id` trailer, a duplicate
   trailer within the walk, a `fixup!`/`squash!` subject, or a commit-sha
   already recorded under a different change. The walk is all-or-nothing — a
   half-valid walk would record a chain shape the reviewer can't trust;
   nothing is recorded, the agent fixes locally and re-pushes.
4. The push is rejected `409` if it would record a new revision for an
   **abandoned** change: an abandoned change must be **reopened explicitly**
   first (`nit reopen`), so a stray re-push never silently resurrects it.

A push that walks to nothing (`tip` is ancestor-or-equal of `base`) is valid
and writes nothing — an empty chain.

### Per commit: upsert change, append revision

For each walked commit, oldest-first, with `parent_sha` = the previous
walked commit's sha (or `fork` for the first):

- **Upsert the change.** `INSERT INTO change (repo_id, change_key) … ON
CONFLICT DO NOTHING RETURNING id`, falling back to a `SELECT`. The UNIQUE
  index makes this idempotent and self-serializing.
- **Append a revision iff the content moved.** Compare the commit-sha to the
  change's latest revision sha. Equal ⇒ a no-op. Differ (or no revision yet)
  ⇒ append a `revision` entry to _this change's_ log:

  ```jsonc
  {
    "number": 2,
    "commit_sha": "…",
    "parent_sha": "…",
    "base_sha": "<fork sha>",
    "partial": true,
    "resets_status": true,
  } // false only for a pure rebase (patch-id-equal
  // AND message unchanged), per change
  ```

`resets_status` is per change: a reword is reviewable as `/COMMIT_MSG`, so it
resets to `pending`; a pure rebase does not.

### Idempotency and reopen

A re-push where **nothing moved** — the tip sha already equals the change's
latest revision — appends nothing and is **rejected** (`409 nothing to
push`), so the agent learns the push was empty rather than believing it
landed work. The storage layer stays idempotent (re-applying the same
`(change_id, idx)` is a no-op), so a crash-retry is safe; only the API
response is explicit. A change appearing in two pushes on different parents
is two revisions of one change, not a conflict, and pushes normally.

A **merged** change is a closed state; a re-divergence is observed by the
timer, not the push path. An **abandoned** change is gated: a push that would
add a revision to it is rejected (step 4 above) and the agent must
`nit reopen` first. Closure is never overridden silently — reopen is an
explicit act.

### Atomicity

A push touching N changes commits them in **N per-change transactions**,
oldest-first. A push is **not atomic across changes** — a crash or a
concurrent reader mid-push can see some changes recorded and others not. This
is a tolerated non-guarantee, made safe by construction: `chain_from_tip`
truncates on an unresolved `parent_sha` (a torn push renders a partial chain,
never an error), and the tip-commit set is updated only after the tip's own
revision commits — a half-written push exposes no tip. Whole-push atomicity
is rejected — it would force the cross-push deadlock or a batched post-commit
apply, both more complex than ordering the commits.

### Returns

```jsonc
PushResult = {
  "tip_change": {"change_id": 10, "change_key": "I3f2…",
                 "revision": 2, "status": "pending"},
  "chain": Chain     // tip-rooted; the ordered path with each member at the
                     // revision this push gave it (web_url selects the tip)
}
```

There is no chain id — a chain is addressed by its tip change id plus an
optional `?revision` selecting the patchset (hence the chain context).

## Events, cursor, and the CLI

A change owns a log; a chain is a path over a set of changes. A follower
watches a **set** of changes over **one websocket**, choosing its own
membership; the server tracks only the subscription set — no per-follower
chain, no resubscribe bookkeeping.

### The change stream — a client-driven websocket

`WS /api/stream?repo={id}` replaces the old per-chain SSE endpoint; the client
drives membership over the open socket:

```jsonc
// client → server
{"subscribe":   {"10": 4, "11": 0}}   // change_id → from-idx: replay [from, head) then stream live
{"unsubscribe": [13]}                 // drop a change
// server → client
{"change_id": 10, "idx": 5, "seq": 412, "kind": "review", "created_at": "…", "payload": {…}}
{"new_parent": {"of": 10, "parent": 9}}    // out-of-log: 10's tip re-rooted onto change 9
```

The server emits **only** entries for currently-subscribed changes. It joins
their per-change in-memory event streams in a **keyed dynamic-membership map**
(`tokio-stream`'s `StreamMap`, which keys substreams by `change_id` and
supports `remove` — the live-substream removal `futures`' `SelectAll` lacks).
A `subscribe` inserts a change's broadcast receiver and replays its
`[from, head)` backlog; an `unsubscribe` removes its key; an append fans out
only to sockets subscribed to that change. The connect-time
**arm-before-backlog** discipline holds per mid-socket subscribe: arm the
receiver **before** reading `[from, head)`, then drop live entries with
`idx < last_backlog_idx + 1` — the arm/read overlap is a duplicate the
watermark suppresses, never a gap. There is no per-chain broadcast channel and
no server-side chain — following a whole chain is the client subscribing to
each member over the one socket; the server never assembles it.

### Two indices on every entry

Every log entry carries both coordinates (Entities and schema):

- **change-local `idx`** — the fine-grained resume; the `subscribe` map keys
  each change to its resume idx (`{"subscribe": {"10": 4}}` = "this change
  from entry 4"). This is the coordinate the UI uses if it later goes
  event-based.
- **global `seq`** — the repo-wide monotone order. It sorts the aggregated
  chain log, and it is the coarse whole-set resume coordinate: a follower
  hands the server a single `seq` (`nit log --follow <change> <seq>`, or
  `nit wait` resuming after a restart) and the server expands it to each
  subscribed change's first entry at or after that `seq` — resume the entire
  watch set from one number, no per-change idx bookkeeping needed.

### The vector cursor

```json
{ "10": 4, "11": 2, "13": 0 }
```

A map from `change_id` (string key) to the 0-based count of that change's log
entries already consumed.

- **Absent key ⇒ 0.** A change newly stacked into a chain is absent from the
  cursor, so it replays from idx 0 — the whole losslessness argument for
  membership change. (The `subscribe` frame expands an absent key to an
  explicit `0`; absent-key defaulting is a cursor convention, not a wire one.)
- **Stale keys are inert.** A change that left the path keeps its key; it is
  never subscribed, never consulted, costs nothing.
- **Monotone per change.** Each value only advances, set to that change's
  head after a drain. The vector is the canonical resume state; a global
  `seq` is the coarse alternative the server expands (above), and the
  aggregated chain log's single timeline is ordered by `seq`.

### The "new parent" message

The **only** non-log message. When a change's tip re-roots onto a new parent
(the agent stacked a commit or re-parented the chain), the server emits
`{"new_parent": {"of": 10, "parent": 9}}` to sockets subscribed to the
affected change; the client reacts by subscribing to the new parent and
re-deriving its logical chain. **Chain-state tracking and resubscription live
in the client**, never the server. It is out-of-log — no `idx`, no `seq`,
advancing no cursor slot — and has no ordering relationship to any change's
backlog replay: it may arrive at any point, is purely advisory, and is
idempotent (the next HEAD re-derivation supersedes it), so a dropped one costs
nothing.

### `nit wait` — watch set from local HEAD

Called from a worktree. Each call:

1. Walk local `HEAD` to the merge-base, collecting each commit's
   `Change-Id`; resolve each to its `change_id`. An unresolvable Change-Id
   (a built-but-unpushed tip) is **skipped** — the watch set is the
   intersection of local-HEAD changes and server-known changes, so a wait
   before the tip is pushed watches the pushed prefix.
2. Open the socket and `subscribe` the watch set with the vector cursor;
   consume the replayed `[from, head)` backlog, advancing the cursor.
3. Apply the wake rule to the drained run. If anything wakes, print
   `{cursor, entries, feedback}` and close. Otherwise stay on the socket,
   block on the first live frame, then re-derive HEAD and re-subscribe.

Re-deriving from HEAD each call lets a wait pick up a commit the agent just
stacked, and makes it self-healing — it never depends on a `new_parent`
message.

### `nit log --follow` — a parked monitor

`nit log --follow` anchors on the tip change id — derived once from local
HEAD's tip at start, or given with `--chain`. It survives a sha rewrite
because the change id is stable across revisions.

- **On connect** it re-resolves the anchor's tip, re-derives the path, and
  `subscribe`s the union. A server restart mid-follow is survivable: reconnect,
  re-derive, re-subscribe — the new tip enters the set, the departed change
  goes quiet.
- It relays each tagged entry, advancing `cursor[change_id] = idx + 1`. On a
  `new_parent` message it subscribes to the parent.
- `--reviewer-only` mutes the agent's own entries client-side and applies the
  wake rule per change.

### The wake rule

Every entry wakes **except** a `review` with verdict `approve`, no comments,
that does not bring the **chain** to `approved` (accumulated and handed back
with the next waking entry). The follower applies this client-side: it already
derives its path and each member's pinned revision from local HEAD (see
`nit wait`), so it evaluates the rule against its own chain-state without
server help — what the old design did server-side in `derive_state` is now
this client-side fold. The server ships raw tagged entries.

A `lifecycle` entry **wakes** when it changes the derived chain-state the
agent branches on — including a **prefix merge of an ancestor**: when an
ancestor lands, the agent is woken so it can choose to rebase onto the
advanced canonical branch.

**Deferred — sibling-note suppression.** Both exceptions above are already
path-aware (they key off the follower's own chain-state). The one piece
deferred is muting a sibling-chain `revision`/`comment` `newer_elsewhere`
note — a push or comment on a revision the follower's path does not pin: the
first cut wakes on those and lets the agent triage rather than computing the
suppression. (Open question below.)

### api.md contract delta

- **New** `WS /api/stream?repo={id}` — the client-driven websocket; replaces
  the per-chain SSE events endpoint. Client `subscribe`/`unsubscribe`; server
  ships tagged entries (subscribed changes only) and `new_parent`.
- **New** `GET /api/changes/{id}/log?from={a}&to={b}` → `{"head", entries}` —
  one change's log slice, for one-shot reads (`nit status`); same range rules
  and 400s as the old chain log.
- **New** `GET /api/chains?repo={id}`, `GET /api/chains/{change_id}`,
  `GET /api/chains/{change_id}/log` — the on-demand chain helpers above.
- **Removed** the per-chain SSE events endpoint.

```jsonc
TaggedLogEntry = {"change_id": 10, "idx": 5, "seq": 412, "kind": "…", "created_at": "…", "payload": {…}}
ClientMsg      = {"subscribe": {"<id>": <from-idx>, …}} | {"unsubscribe": [<id>, …]}
NewParent      = {"new_parent": {"of": 10, "parent": 9}}
```

The scalar `nit wait <n>` form is gone; `nit wait <cursor-json>` and
`nit log --follow <cursor-json>` take and print the vector; either also
accepts a global `seq` scalar, which the server expands to each change's
resume idx.

## Lifecycle

Lifecycle is a property of the **change**. Status, mergedness, partial and
abandonment are folded from the change's log and read per the patchset a path
pins. A chain's lifecycle state is a pure read-time function of its members.

### Per-change, per-revision status

A change's **displayed status** is per `(change, revision)`: the verdict of
the latest review whose `revision` equals the patchset a path pins, falling
back to `pending`. There is no change-wide status scalar — two tips pinning
two patchsets carry two independent verdicts, so an approve in the C-chain
never overwrites a request_changes in the E-chain.

```
displayed status:  pending | approved | changes_requested | commented
                   | merged | abandoned
```

`merged`/`abandoned` are terminal for review. `merged` re-diverging is the
timer's business; `abandoned` only clears on an explicit reopen.

### Review verdicts in a multi-chain world

A review targets `(change, revision)`. **Stale** = walked by no tip. The
endpoint accepts a review of any patchset some tip currently pins and lands
the verdict on that pair; it rejects only a truly detached patchset.
Auto-retarget-to-latest is removed — with two live patchsets "latest" is not
unique, and retargeting would move a verdict across chains. `counts.unresolved`
and the feedback signal are scoped to threads anchored at the pinned revision
(thread display is already `(revision, side)`-pinned).

### The per-change merge timer

A per-repo background timer sweeps each live change, matching its latest
revision against the **canonical branch** — the one base there is.

1. **Fork point** = the revision's `parent_sha`; the window is
   `fork..canonical`.
2. **Change-Id match**: a commit in `fork..canonical` carries this change's
   `change_key` — _and_ its patch-id matches the change's current latest
   revision. A trailer in canonical whose landed patch-id differs from the
   current revision is "previously landed, now amended": the change stays open
   (surfaced with a note), preventing the Change-Id-only oscillation.
3. **Patch-id match**: else the change's diff patch-id appears among the
   `fork..canonical` patch-ids. An empty diff never alone counts as a landing.

The timer is the **only** writer of `merged`, recording
`lifecycle{merged, revision}` — which patchset landed (into the canonical
branch). A push never writes merged (it cannot observe the base advancing).
Prefix merge falls out for free: landing A and B marks them merged while C
stays live, with no chain-level all-or-nothing gate.

Mergedness is consulted **per (change, pinned-revision)**: a path pinning a
non-landed patchset of a merged ancestor shows that member live with a "newer
revision landed elsewhere" note, not merged. A path pinning the landed
revision shows it merged.

### Abandon and reopen

A change is **abandoned** when its keep-ref'd latest revision is unreachable
from **any** tip across **two consecutive sweeps ≥ 10s apart** (the gap rides
out mid-rebase windows; the timer is in-memory, so a restart resets it —
abandonment is best-effort, delayed not wrong). Reachability is set-membership
over the tip-commit set, refreshed each sweep:

- A change shared by two chains is reachable as long as **either** tip walks
  it. Deleting tip C while E is live leaves B reachable from E.
- "Abandon a chain" is not a primitive — dropping a tip drops the changes
  reachable _only_ through it; the rest persist.

**Reopen is explicit.** An abandoned change does not auto-reopen on bare
re-reachability, and a push that would add a revision to it is rejected (Push
step 4) with a "reopen first" indication. `nit reopen <change>` appends a
`lifecycle{reopened}`, clearing `abandoned` back to the retained verdict
status; the agent may then push a new revision (which folds it to `pending`).
Making reopen deliberate keeps a transient re-reachability — a rebase that
briefly re-touches an abandoned commit — from silently resurrecting it.

### Keep refs

```
refs/nit/keep/<change-id>/<revision-number> → revision commit
```

One ref per revision of every non-terminal change pins its objects. GC is
**reference-counted across changes**: a revision's keep ref is deleted only
when its change is terminal **and** no live revision records its `commit_sha`
as a `parent_sha`. Every commit that is the parent of any live revision stays
pinned even if its own change merged — so a prefix-merged ancestor a tip still
walks through keeps its objects, and the SHA-walk, vs-parent diffs, and the
timer's `fork..canonical` walk never dereference a gc'd commit.

### Partial

Each revision carries its push's partial flag. A member's effective partial
is the flag of the revision the path pins. A chain is **partial** iff its
**tip** change's latest revision is partial — the tip is the work frontier,
so the most recent push's intent governs whether the chain may merge. A
shared interior change carrying a stale partial from a sibling push does not
hold an unrelated chain partial. `nit ready` re-stamps the tip's latest
revision to `false`. Partial blocks the `approved` state; it never blocks
review.

### Derived chain state

Computed over the members on the tip's walk, each at the revision that tip
pins:

```
every member merged (at its pinned revision)       → merged       (off the main page)
else any member abandoned                          → has_abandoned (shown, flagged)
else any member changes_requested|commented        → agents_turn
else any member pending                            → waiting_for_review
else all approved (≥1)                             → agents_turn if partial, else approved
else (no members)                                  → agents_turn   (empty tip)
```

A chain drops off the main page iff **every** member is terminal at its
pinned revision. Any one live member keeps a partially-landed stack visible.
Tip-merged-but-ancestor-live keeps the chain — membership, not the tip, is
the rule.

### Dangling and empty

- **Dangling change** (in no tip, not terminal): off every chain view,
  retained and pinned, judged only by the sweeps; surfaced again when a push
  walks through it.
- **Empty tip** (walk empty): derives `agents_turn`, listed while its tip
  commit is known, dropped when no ref names it and the tip-commit set is
  recomputed.

## Concurrency

State is the fold of per-change append-only logs. The unit of contention is
**one change's log**. A chain owns no log and no lock. SQLite is the single
writer: WAL, `BEGIN IMMEDIATE`, `busy_timeout`, one connection per blocking
task, no `Mutex<Connection>`.

Each loaded change holds `proj: StdRwLock<ChangeProj>` — the fold — and **no
async mutex**.

### Chain birth: eliminated

Creating a change is `INSERT INTO change (repo_id, change_key) … ON CONFLICT
DO NOTHING RETURNING id`, guarded by `UNIQUE(repo_id, change_key)`. Two pushes
first-seeing the same key race one SQLite-serialized statement; the loser is a
no-op and both read the same `change_id`. No cross-change read-decide-insert,
no owner routing, no per-repo lock. The coordination is subsumed by an index
constraint — nothing relocates.

### Append ordering: SQLite orders commits, a reorder buffer orders applies

An append to change `C` runs inside `spawn_blocking`. SQLite's single writer
assigns a contiguous `idx` and the global `seq` under `BEGIN IMMEDIATE`; the
`UNIQUE (change_id, idx)` makes a duplicate a hard error. **No std lock is
held across the commit.** After the commit, the appender takes `C`'s
`proj.write()` for the microsecond apply only:

```
let tx = conn.transaction(BEGIN IMMEDIATE);
let idx = MAX(idx)+1 for C;        // committed-state read under the write lock
validate: fold the entry onto a clone of C's projection;   // may abort, nothing written
append_log(C, idx, …); tx.commit();                        // releases the SQLite lock, mints seq
// --- apply, ordered without a lock across I/O ---
let mut p = C.proj.write();        // microsecond guard, no .await, no commit inside
if entry.idx == p.head { fold(&mut p, entry); drain any contiguous stashed entries }
else { stash entry until p.head reaches it }
drop(p); publish(entry);           // after durable commit + fold
```

The **reorder buffer** applies an entry iff `entry.idx == proj.head`, else
stashes it and drains contiguously — so apply-order == idx-order ==
commit-order == replay-order, even when two pushes race a revision onto the
same change. `revision_number` is assigned in the fold from projection state,
so it too tracks idx order. SQLite contiguous idx guarantees no gaps, so the
pending map is shallow. The held guard spans only the in-memory fold
(microseconds), never the blocking commit — so readers of `C` never stall on a
writer's I/O, and no `{change-guard, SQLite-write-lock}` cycle can form.

Cross-change appends never contend: two changes hold two different std guards
and two disjoint transactions. No appender ever holds two change guards at
once (the invariant that keeps a multi-change push deadlock-free).

### Chain-view assembly is lock-free

A chain view walks `parent_sha` change-by-change, taking each change's
`proj.read()` in turn, never two at once — deadlock-free, never blocking an
append to a change it isn't holding. The view is a snapshot of per-change
snapshots: it can observe B at rev1 while a concurrent push appends B's rev2,
which is correct (it reflects the tip that was pushed). No global lock orders
the composition because the chain is derived.

### What survives — a channel, not a mutex

Per-change broadcast channels and the **arm-before-backlog** discipline: a
subscriber arms every change in its watch set **before** reading any backlog,
deduping per change by the idx watermark, so no append slips the arm/read gap.
Publish is strictly after the durable commit and the in-memory fold;
`try_broadcast` stays non-blocking (overflow on), so a slow subscriber never
stalls an appender. The websocket joins these per-change channels in a keyed
`StreamMap` (`tokio-stream`), arming each receiver on `subscribe` before it
replays the backlog and watermark-deduping the arm/read overlap — new
machinery, born of per-change logs, not relocated from a lock.

### Verdict

Both async mutexes are removable. The **per-repo chain-birth lock** is
eliminated outright — a UNIQUE-constrained upsert subsumes it, nothing
relocates. The **per-chain append gate** is replaced by a synchronous
per-change reorder buffer: std not tokio, one change not a whole chain, no
`.await` held across it. The ordering it provides (in-memory applies matching
commit order) is real and does not vanish — SQLite orders commits, not the
applies that follow — but a shallow reorder buffer discharges it with no lock
spanning I/O. The write path is strictly simpler; the read/subscribe path
gains the vector cursor and the client-driven websocket. That is relocation of
complexity from server write-path to client read-path, stated plainly, not
pure removal.

## Web and API contract delta

A chain is addressed by its **tip change id** plus an optional `?revision`;
there is no chain id and no stored branch key. Clients reach chains through
the on-demand helpers, and a tip's display name is resolved server-side at
query time.

- `GET /api/repos` → `Repo` carries `base_branch` and `active_chains`
  (tip-commit count, derived).
- `GET /api/chains?repo={id}` → `[ChainSummary]` — one entry per tip commit,
  each with its derived `path` and best-effort `name`; `status=all` includes
  merged/abandoned.
- `GET /api/chains/{change_id}` → `Chain` — the derived `path` through that
  change's tip.
- `GET /api/chains/{change_id}/log` → the aggregated chain log, sorted by
  global `seq`.
- `GET /api/changes/{id}` → `ChangeDetail` — no `chain_id`, no `position`
  (both are path properties). Adds `parent_sha`/`base_sha` per revision and
  `chains: [ChainRef]` (every tip walking through this change, each with the
  patchset it pins).

```jsonc
PathEntry = {
  "change_id": 10, "position": 0, "change_key": "I3f2…",
  "revision": 2,            // the patchset this path walks
  "latest_revision": 3,     // the change's newest patchset anywhere
  "newer_elsewhere": true,  // latest_revision > revision (badge driver)
  "status": "pending",      // per (change, this revision)
  "merged_elsewhere": false,// a newer revision landed on the canonical branch
  "subject": "…", "commit_sha": "…",
  "counts": {"threads": 3, "drafts": 1, "unresolved": 2}  // scoped to this revision
}
ChainRef = {"tip_change_id": 12, "revision": 2, "name": "feat/x", "web_url": "…"}
```

`position` is a property of a **path**, never of the change — two chains place
the same change differently. `status`, `unresolved`, and `state` are read at
the path's pinned revision. `ChangeDetail.reviews` and `.threads` are
change-wide and carry their `revision`; clients MUST filter by the viewing
`?revision`.

### `?revision` selects chain context

The change page reads `?revision=N` (default: latest). The selected patchset's
`parent_sha` determines the path, so `?revision` implicitly selects the chain
context — there is no `?chain` param. The diffbar's right select is
`revision`; switching it re-roots the breadcrumb on the new patchset's chain.
A `newer_elsewhere` row opens at the patchset **this chain pins**, not the
change's latest, so the reviewer sees the change as it stood in the chain they
came from.

### Frontend

- `/` Repos — `active_chains` derived; `base_branch` shown.
- `/repos/:id` Dashboard — one row per tip from `GET /api/chains`, each with
  its query-time name; merged/abandoned drop off; a partially-landed stack
  renders mixed-terminal members inline.
- `/chains/:change_id` Chain — renders the derived `path`; a `newer_elsewhere`
  row carries the cross-chain badge; no orphaned-collapsed section.
- `/changes/:id` Review — reads `?revision`; the breadcrumb is derived from
  `chains` for the selected patchset; cross-chain badge when other tips pin
  this change at a different patchset.

Mock fixtures gain the shared-change scenario (two tips through one change at
two patchsets) so `newer_elsewhere`, the badge, and `chains`/`ChainRef` render
without a backend; `mockRequest` derives the `path` from fixture `parent_sha`
chains rather than a stored list.

## Migration

A **clean cut**. The old `(chain_id, idx)` logs, the `chains` table, and
`refs/nit/keep/<chain>/…` are non-portable. Re-push live branches into the
change-centric model (git is the source of truth for every live change);
closed chains lose their stored record. No dual-read fold path, no
translation step — history is not a concern for this cut.

## Open questions

- **Already-registered push rejection.** A no-op re-push (tip sha already the
  change's latest) is rejected `409` rather than a silent success — confirm
  this doesn't trip a crash-retry that expects idempotent success (the storage
  layer is still idempotent; only the response differs).
- **Tip-set rebuild on restart.** The in-memory tip-commit set rebuilds from
  keep refs after a restart. Confirm a keep'd-but-since-deleted-branch tip
  should still list (best-effort name falls back to subject) until the next
  sweep prunes it.
- **Sibling-note wake suppression (deferred).** The first cut does not mute
  sibling-chain `revision`/`comment` notes (it wakes on them); the approve and
  lifecycle exceptions are already path-aware. Suppressing activity on a
  revision the follower's path does not pin is a later refinement — confirm an
  agent triaging the occasional sibling-chain wake is acceptable until then.
- **Sweep cost.** The chains page SHA-walks each tip per load, and the per-repo
  timer walks `fork..canonical` per change. Confirm this is acceptable or needs
  the current scan's throttle/cache.
