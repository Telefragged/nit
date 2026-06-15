# Architecture

nit is a single-machine, local-first review server. Three parts:

1. **`nit` binary** (`crates/nit`) — Rust. One executable with subcommands:
   - `nit serve` — axum HTTP server: JSON API under `/api`, serves the built
     web UI (static files from `--web-dist` / `$NIT_WEB_DIST`) for everything else.
   - `nit push` / `nit wait` / `nit log` / `nit status` — thin CLI clients
     of that API, run by coding agents from inside a git repo.
2. **Web UI** (`web/`) — React/TS SPA built with Vite. Talks only to `/api`.
3. **State** — an append-only **event log** per chain, in SQLite. The
   server folds each log into an in-memory state machine (rebuilt by
   replaying the log on startup) and serves reads from it. Git data is
   **never copied** and diffs are **never stored**: the server computes
   commits/diffs directly from the registered repos with git2, on demand.

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

- **Local-first**: server and agents share a filesystem. No auth; binds 127.0.0.1.
- **Repos are read in place** via libgit2, grouped under a minimal **repo
  registry** keyed by the git-common-dir (the `.git` dir — one identity per
  repo, shared across its worktrees; relocated with `nit repo move`).
  Registering a chain stores only `(repo_id, branch name, base ref)`; the
  repo row holds just its git-common-dir, nothing else git already knows.
- **State is the fold of an append-only log**: every reviewable fact —
  a pushed revision, a verdict, a reply, a partial flip, a merge — is one
  immutable log entry. The chain's current state is the fold of its log,
  held in memory and rebuilt by replay on startup. Nothing is mutated in
  place, so history is total and re-derivable; SQLite stores only the log
  (see [data-model.md](data-model.md)).
- **Rescan-on-read, but safe**: pushes and (throttled) dashboard/chain GETs
  re-walk `base..tip`, and a scan that changed structure **appends one
  `revisions` entry** (no-op otherwise, so read-scans never bloat the
  log). A walk is milliseconds; no file watchers. Merged/deleted branches
  append `chain_closed`. Every appender serializes through a per-chain
  lock in a single transaction; scans never destroy review data (changes
  are orphaned, not deleted) and a failing chain never breaks the others
  (data-model.md "Concurrency").
- **Unit of review is the commit** (a "change"), grouped in a "chain"
  (one registered branch). The required `Change-Id:` trailer is the
  identity — see [data-model.md](data-model.md).
- **Amends become revisions**: rewriting a commit (same `Change-Id:`
  trailer, new sha) appends a revision to its change, gerrit patchset
  style. Revision history is pinned against `git gc` by
  `refs/nit/keep/*` refs.
- **Drafts live server-side** so the reviewer can move between commits and
  sessions without losing them; they are the one piece of mutable state
  kept outside the log (their own table) and publish atomically into one
  `review` entry with a verdict.
- **The log is the cursor stream** powering `nit wait`: the agent owns a
  0-based cursor and subscribes for entries beyond it, so no entry is ever
  missed between calls and answering with replies alone can't spin
  (data-model.md "Wake rule"; api.md "events").

Deeper reading: [data-model.md](data-model.md) (schema, scan algorithm),
[api.md](api.md) (the HTTP contract), [frontend.md](frontend.md) (UI),
[agent-workflow.md](agent-workflow.md) (how agents drive nit),
[dev.md](dev.md) (dev loops, testing, nix).
