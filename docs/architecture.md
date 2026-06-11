# Architecture

nit is a single-machine, local-first review server. Three parts:

1. **`nit` binary** (`crates/nit`) — Rust. One executable with subcommands:
   - `nit serve` — axum HTTP server: JSON API under `/api`, serves the built
     web UI (static files from `--web-dist` / `$NIT_WEB_DIST`) for everything else.
   - `nit push` / `nit wait` / `nit status` — thin CLI clients of that API,
     run by coding agents from inside a git repo.
2. **Web UI** (`web/`) — React/TS SPA built with Vite. Talks only to `/api`.
3. **State** — SQLite database (review state only). Git data is **never copied**:
   the server reads commits/diffs directly from the registered repos with git2.

## Dataflow (the review loop)

```
agent                         nit server                      reviewer (browser)
  |  nit push  ──────────────▶  scan branch, upsert chain      |
  |                             changes + revisions in db      |
  |  nit wait (long-poll) ───▶  blocks on event cursor         |
  |                                                            |
  |                             ◀── browse dashboard/diffs ────|
  |                             ◀── draft comments (stored) ───|
  |                             ◀── submit review (verdict) ───|
  |  ◀── feedback JSON ───────  event fires, wait returns      |
  |  fix, commit fixup!,                                       |
  |  nit push  ──────────────▶  new revision, status→pending   |
  |  ... repeat until approved; agent rebases/merges;          |
  |  next scan detects merge → chain leaves the dashboard      |
```

## Key decisions

- **Local-first**: server and agents share a filesystem. No auth; binds 127.0.0.1.
- **Repos are read in place** via libgit2. Registering a chain stores only
  `(repo path, branch name, base ref)`.
- **Rescan-on-read, but safe**: pushes and (throttled) dashboard/chain GETs
  re-walk `base..tip` and reconcile the db. A walk is milliseconds; no file
  watchers. Merged/deleted branches are detected the same way. Scans and
  review submissions serialize through a per-chain lock in single
  transactions; scans never destroy review data (changes are orphaned, not
  deleted) and a failing chain never breaks the others (data-model.md
  "Concurrency").
- **Unit of review is the commit** (a "change"), grouped in a "chain"
  (one registered branch). Change identity survives rebases — see
  [data-model.md](data-model.md).
- **Fixups fold into revisions**: a `fixup!` commit becomes a new revision of
  the change it targets (in-memory tree merge), gerrit patchset style.
  Attachment mirrors `git rebase --autosquash` exactly; folded trees are
  pinned against `git gc` by `refs/nit/keep/*` refs.
- **Drafts live server-side** so the reviewer can move between commits and
  sessions without losing them; they publish atomically with a verdict.
- **Events table = cursor stream** powering the `/wait` long-poll; clients
  act on the feedback snapshot it returns, never on raw events, so wakeups
  can't be missed between calls.

Deeper reading: [data-model.md](data-model.md) (schema, scan algorithm),
[api.md](api.md) (the HTTP contract), [frontend.md](frontend.md) (UI),
[agent-workflow.md](agent-workflow.md) (how agents drive nit),
[dev.md](dev.md) (dev loops, testing, nix).
