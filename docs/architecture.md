# Architecture

nit is a single-machine, local-first review server. Three parts:

1. **`nit` binary** (`crates/nit`, Rust) — one executable:
   - `nit serve` — axum HTTP server: JSON API under `/api`, serves the
     built web UI (`--web-dist` / `$NIT_WEB_DIST`) for everything else.
   - `nit push` / `status` / `log` / `comment` / `abandon` / `reopen` —
     thin CLI clients of that API, run by agents from inside a git repo.
2. **Web UI** (`web/`) — React/TS SPA (Vite). Talks only to `/api`.
3. **State** — an append-only **log per change** in SQLite, folded into an
   in-memory per-change state machine (rebuilt by replaying the log on
   startup). Git data is never copied and diffs never stored: commits and
   diffs are computed from the registered repos with git2 on demand.

## Components

- **axum server** (`api/`) — every endpoint of [api.md](api.md). All
  rusqlite/git2 work runs in `spawn_blocking`. A background lifecycle timer
  runs alongside `serve` (`api/mod.rs` `run_lifecycle_timer`).
- **git layer** (`gitscan/`) — pure with respect to the database: the push
  walk (`walk_push`), merge detection (`landed_revision`), query-time tip
  names (`tip_name`), patch-ids and keep refs (`gitscan/objects.rs`). It reads
  git and returns values the api layer folds.
- **SQLite** (`db/`) — the four-table log; nothing in it is ever mutated or
  deleted. See [data-model.md](data-model.md) "Tables".
- **The per-change fold** (`nit_types::fold`) — `ChangeProj` is one change's
  folded state; `fold` applies one wire `LogEntry`, `replay` rebuilds a change.
  Revision numbers and thread ids are minted **in the fold**. It is **pure over
  `nit-types`** — no database, serialization, or publishing — so the same code
  runs on the server and, compiled to WebAssembly (`crates/nit-wasm`), in the
  browser. The `nit` crate's `review/` holds only the db/storage adapters
  (`entry_from_row`, `replay_rows`, the `log.payload` column split).
- **Chain derivation** (`chain/`) — `RepoView` resolves a commit-sha to
  `(change, revision)` and walks a tip back to the base. A pure function of
  owned change snapshots; holds no locks, touches no git.
- **React SPA** (`web/`) — the reviewer's browser UI ([frontend.md](frontend.md)).
  The change page is **event-driven**: it subscribes over `WS /api/stream` for a
  `ChangeProj` snapshot and folds the live tail with `crates/nit-wasm` (the
  shared fold), so the browser resumes the server's projection rather than
  reimplementing it.
- **The CLI** — `nit push` and the agent read/comment commands
  ([agent-workflow.md](agent-workflow.md)).

## Dataflow (the review loop)

```
agent                         nit server                      reviewer (browser)
  |  nit push  ──────────────▶  walk merge-base(base,tip)..tip; |
  |                             per change, append a `revision`  |
  |                             entry iff its sha moved; fold     |
  |                             ◀── browse dashboard/diffs ────|
  |                             ◀── draft comments (stored) ───|
  |                             ◀── submit review (verdict) ───|
  |  nit status ─────────────▶  drains one change's drafts into  |
  |  (one-shot read)            one `review` entry, fold; the     |
  |  ◀── log slice + state ───  change's status at that revision  |
  |  fix, amend, nit push ───▶  → the verdict; status→pending     |
  |  ... repeat until approved; agent rebases/lands;            |
  |  a background timer marks landed changes `merged` → off it   |
```

Reads derive chains on demand: a `chains` GET snapshots every change of a
repo into a `RepoView`, takes its tip set, and walks each tip's `parent_sha`
to the fork on the canonical branch (`chain/` `path_from_tip`). Nothing about
a chain is stored or scanned at read time.

Live followers (`nit log --wait`/`--follow`) watch a set of changes over
one websocket (`WS /api/stream`); the server joins the subscribed changes'
per-change broadcast channels in a `tokio-stream` `StreamMap`, and the wake
rule is a client concern (docs/data-model.md). The web polls the same folds.

## Key decisions

- **Local-first**: server and agents share a filesystem. No auth; binds
  127.0.0.1.
- **Change-centric**: the **change** (a `Change-Id`, scoped to a repo) is the
  primary entity. It owns an append-only log whose fold is its entire
  reviewable state — revisions, threads, reviews, lifecycle. A
  **chain** is never stored, in SQLite or in memory; it is derived at read
  time by walking a tip commit back to the repo's canonical branch through
  each revision's recorded `parent_sha` (gerrit relation chains). The same
  `Change-Id` reached by two pushes on different parents is one change with
  two patchsets, surfaced as two chains.
- **State is the fold of an append-only log**: every reviewable fact (a
  revision, verdict, comment, lifecycle transition) is one
  immutable entry; the current state is the fold, held in memory per change
  and rebuilt by replay on startup. SQLite stores only the four tables
  ([data-model.md](data-model.md)); revision data lives in `revision`-kind
  entries — there is no revision table.
- **No stored chains, order, or position**: order is the read-time
  `parent_sha` walk; a member's position is a property of the path it sits in,
  not of the change. Two chains place the same change differently and carry
  independent verdicts (a request_changes in one never overwrites an approve
  in another). There is no chains table and no `position` fold machinery.
- **Push is the only writer of revisions**: it walks `merge-base(base,
tip)..tip` oldest-first, upserts each change by its `Change-Id`, and appends
  a `revision` entry **iff the commit-sha moved** (a pure rebase — patch-id-
  and message-equal — appends a revision but does not reset review status).
  The walk is all-or-nothing (a `400` on any structural fault), a revision to
  an abandoned change is a `409`, and a repo has exactly **one canonical
  branch** ([data-model.md](data-model.md) "Push"). There are **no read-time
  scans** — a read never walks git to discover structure.
- **The lifecycle timer** (`api/mod.rs` `run_lifecycle_timer`) is the only
  writer of `merged`: per repo it sweeps each live change, marking it `merged`
  when its patch lands on the canonical branch (`landed_revision`). A push
  cannot observe the base advancing, so it never writes merged. Abandonment is
  an explicit action (`nit abandon`), not a sweep; `nit reopen` clears it.
  `NIT_TIMER_INTERVAL_MS` configures the sweep interval.
- **The unit of review is the commit** (a "change"); the required
  `Change-Id:` trailer is its identity. Amends become revisions (gerrit
  patchset style), pinned against `git gc` by
  `refs/nit/keep/<change-id>/<revision-number>` refs — one per revision,
  idempotent (GC/deletion is deferred in this cut; refs accumulate, fail-safe).
- **Reviewer drafts live server-side** so the reviewer can move between commits
  and sessions without losing them — the mutable state outside the log. Comment
  drafts and a per-change **staged decision** (`draft_reviews`: a verdict or an
  abandon/reopen) both publish atomically: a chain's batch submit folds each
  member's decision and drained comment drafts into one per-change transaction
  (docs/data-model.md "Reviewer decisions").
- **The per-change append lock, no async mutex**: each loaded change holds
  `proj: RwLock<ChangeProj>` (the fold) and a synchronous append lock
  (`StdMutex`) serializing its appenders. An append validates the fold on a
  throwaway copy before committing under `BEGIN IMMEDIATE`, then folds. The
  contention unit is one change's log — cross-change appends never contend,
  and no appender ever holds two change locks at once
  ([data-model.md](data-model.md) "Concurrency").

Deeper reading: [data-model.md](data-model.md) (schema, fold, chain
derivation, lifecycle), [api.md](api.md) (the HTTP contract),
[frontend.md](frontend.md) (UI), [agent-workflow.md](agent-workflow.md) (how
agents drive nit), [dev.md](dev.md) (dev loops, testing, nix).
