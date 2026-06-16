# Architecture

nit is a single-machine, local-first review server. Three parts:

1. **`nit` binary** (`crates/nit`, Rust) — one executable:
   - `nit serve` — axum HTTP server: JSON API under `/api`, serves the
     built web UI (`--web-dist` / `$NIT_WEB_DIST`) for everything else.
   - `nit push` / `wait` / `log` / `status` — thin CLI clients of that API,
     run by agents from inside a git repo.
2. **Web UI** (`web/`) — React/TS SPA (Vite). Talks only to `/api`.
3. **State** — an append-only **event log** per chain in SQLite, folded
   into an in-memory state machine (rebuilt by replaying the log on
   startup). Git data is never copied and diffs never stored: commits and
   diffs are computed from the registered repos with git2 on demand.

## Dataflow (the review loop)

```
agent                         nit server                      reviewer (browser)
  |  nit push  ──────────────▶  scan branch; append a          |
  |                             `revisions` log entry, fold it  |
  |  nit wait <cursor> ──────▶  return entries [cursor, head);  |
  |                             block while caught up           |
  |                             ◀── browse dashboard/diffs ────|
  |                             ◀── draft comments (stored) ───|
  |                             ◀── submit review (verdict) ───|
  |  ◀── entries + state ─────  append `review` entry, fold,    |
  |  fix, amend commit,         wake the parked poll            |
  |  nit push  ──────────────▶  append `revisions`, status→pending
  |  ... repeat until approved; agent rebases/merges;          |
  |  next scan appends `chain_closed{merged}` → off dashboard   |
```

## Key decisions

- **Local-first**: server and agents share a filesystem. No auth; binds
  127.0.0.1.
- **Repos read in place** via libgit2, grouped under a minimal **repo
  registry** keyed by the git-common-dir (one identity per repo, shared
  across its worktrees; relocated with `nit repo move`). A chain stores
  only `(repo_id, branch, base ref)`.
- **State is the fold of an append-only log**: every reviewable fact (a
  revision, verdict, reply, partial flip, merge) is one immutable entry;
  the current state is the fold, held in memory and rebuilt by replay on
  startup. SQLite stores only the log (see [data-model.md](data-model.md)).
- **Rescan-on-read, but safe**: pushes and throttled dashboard/chain GETs
  re-walk `base..tip`; a structural change appends one `revisions` entry
  (no-op otherwise, so read-scans never bloat the log). Every appender
  serializes through a per-chain lock in one transaction; scans orphan
  changes, never delete them, and a failing chain never breaks the others
  (data-model.md "Concurrency").
- **Unit of review is the commit** (a "change") grouped in a "chain" (one
  branch); the required `Change-Id:` trailer is its identity. Amends become
  revisions (gerrit patchset style), pinned against `git gc` by
  `refs/nit/keep/*` refs. See [data-model.md](data-model.md).
- **Drafts live server-side** so the reviewer can move between commits and
  sessions without losing them — the one mutable state outside the log,
  publishing atomically into one `review` entry.
- **The log is the cursor stream** powering `nit wait`: the agent owns a
  0-based cursor and subscribes for entries beyond it, so nothing is missed
  between calls and replies alone can't spin (data-model.md "Wake rule";
  api.md "events").

Deeper reading: [data-model.md](data-model.md) (schema, scan algorithm),
[api.md](api.md) (the HTTP contract), [frontend.md](frontend.md) (UI),
[agent-workflow.md](agent-workflow.md) (how agents drive nit),
[dev.md](dev.md) (dev loops, testing, nix).
