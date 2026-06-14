# Agent workflow — how coding agents use nit

nit reviews **commits**, not branches. Make each commit one reviewable unit
(one logical change, own subject + body). The branch you register is the
"chain"; every commit on it becomes a change the human reviews separately.

## Conventions for your commits

- Add a `Change-Id: I<unique-token>` trailer (any opaque unique string;
  40 hex like gerrit is customary) to every commit message — and never the
  same token twice. The trailer is **required and canonical**: it is the
  change's identity, keeping its comment history stable across rebases and
  amends; a missing or duplicated trailer fails the scan
  (`last_scan_error`). Keep every trailer in one block — a blank line
  between trailers splits the block and git silently drops the
  `Change-Id`.
- **Never merge into your branch** — no `git pull` without `--rebase`. A
  merge commit in the chain fails the scan; rebase onto the base instead.
- Keep every commit **formatter-clean** (this repo:
  `nix develop -c treefmt`): format before each commit, and after any
  rebase re-format every rewritten commit, not just the tip —
  hand-resolved conflict hunks land unformatted in whichever commit
  conflicted (recipe: docs/dev.md "Formatting"). The reviewer's
  interdiff should show your fix, never whitespace noise.
- Answer review feedback by **amending the reviewed commit in place**,
  keeping its Change-Id, then pushing the rewritten branch. nit appends a
  new revision of the same change; the reviewer sees what you changed
  (interdiff), not a pile of "address review comments" commits.
  `fixup!`/`squash!` commits are fine as a local staging step, but squash
  them before pushing — the scan rejects them (git ≥ 2.44):
  `GIT_EDITOR=true git rebase --autosquash <fork-point>`
  (fallback: `GIT_SEQUENCE_EDITOR=: GIT_EDITOR=true git rebase -i --autosquash <fork-point>`).
- Rewrite onto your chain's **fork point**
  (`$(git merge-base <base> HEAD)`), not the moved base: rebasing
  mid-review drags unrelated base drift into every interdiff. Rebase onto
  the base itself only when you actually need to — landing, or a real
  conflict.

## The cursor — how `nit wait` works

nit's state is an append-only log per chain (docs/data-model.md). You
drive review with a **0-based cursor**: the count of log entries you have
already consumed. It starts at `0`.

```sh
nit wait <cursor>      # returns the entries beyond <cursor>; blocks while caught up
```

`nit wait <cursor>` returns `{head, entries, state, …}`: `entries` are the
log entries you had not yet seen (`[cursor, head)`), and you then set your
cursor to `head`. Two rules make this lossless:

1. **Advance the cursor only from a `wait` (or `nit log`) result** — its
   `head`. Never from `nit push`/`nit reply`: those append entries but
   return no index. If a reviewer comment lands between two of your own
   pushes, jumping the cursor to "after my second push" would skip it;
   only `wait` returns the whole contiguous run, so you always see it.
2. **Keep waiting until it blocks.** Right after you push or reply, the
   next `nit wait <cursor>` returns immediately — it hands back your own
   just-appended entries (and anything interleaved). Process them, advance
   the cursor, wait again; repeat until the call actually blocks. Then you
   are caught up and parked for the reviewer.

Skim entries cheaply with `nit wait --oneline <cursor>` (one line each, so
you can tell your own entries from the reviewer's without token bloat).
Inspect specific entries without moving your cursor with
`nit log <ranges>` (e.g. `nit log 3`, `nit log 3..6`, half-open).

Each entry has a `kind` (docs/data-model.md): `review` (a reviewer
verdict, carrying any thread-resolution changes too — act on it),
`chain_closed` (merged/abandoned — stop), and the ones you caused
(`revisions` from your push, `reply`, `partial`). You act on the
reviewer's entries and on the rolled-up `state` the response carries.

## The loop

**Cadence: pushing is part of completing a commit, not a phase after the
branch.** Commit, push `--partial`, build the next commit. A planned
later pass over the chain (cleanup, self-review, verification) is never
a reason to hold pushes — push now, amend later: post-push amends become
new revisions by design, and the reviewer sees the pass as interdiffs.
The only thing that delays a push is the commit itself not being done.

```sh
cursor=0
# while building — after EVERY completed commit (green, formatter-clean,
#   one concern, Change-Id'd), not once at the end:
nit push --partial            # register/refresh the chain as partial
#   the FIRST push creates the chain — report web_url to the human now;
#   review starts on commit one.
nit ready                     # last commit done: clears partial, refreshes —
                              #   the chain can now reach approved

# then drive the cursor loop until the chain closes:
resp=$(nit wait $cursor)      # blocks until entries land beyond $cursor
cursor=<resp.head>            # advance over everything you just received
# inspect resp.entries (--oneline to skim) and resp.state:
#   for each `review` entry: fix → amend the commit it targets (local
#     fixup! + autosquash onto the fork point), or answer with
#     nit reply <comment-id> [--resolve] -m "…"
nit push                      # the rewritten commits become new revisions
# …then loop: nit wait $cursor again (returns your own entries first),
#   advance, until state=approved or the chain closes
# then: rebase onto <base> if it moved — re-formatting each replayed
#   commit (docs/dev.md "Formatting") — and merge/ff the branch
nit push                      # optional: next scan appends chain_closed{merged}
```

**Running `nit wait` is mandatory, not the optional tail of the loop.**
`nit ready` is never the last thing you do: the instant it returns, a
`nit wait <cursor>` must be running — as a background task — and must stay
running until the chain reaches `approved`, `merged`, or
`abandoned`. A chain left `ready` (or pushed) with no wait parked on it is
a dropped review: the reviewer's feedback lands and nothing ever reacts to
it. Treat "ready/pushed without a wait" as a broken loop, exactly like an
unpushed commit. Re-arm the wait after every push and reply; when it
returns non-actionable (it woke on your own just-pushed entries), advance
the cursor and wait again. The turn is not over while the chain is open —
it is over when the chain closes.

The push duty is **per branch, owned by whoever builds it**. In
multi-agent setups (an orchestrator fanning out workers, one
worktree/branch each) every worker drives `nit push --partial` for its
own branch from its own worktree, starting the moment its first commit
is green — the orchestrator must write that into each worker's
instructions, and must not centralize pushing, batch it, or gate it on
later phases. "Completed" means green and coherent now, not final:
post-push amends become new revisions by design, so a planned follow-up
pass (cleanup, self-review, verification) is no reason to hold the
first push back. From the reviewer's seat, an unpushed branch is
invisible work.

- `nit push [--partial] [--base <ref>] [--branch <name>] [--server <url>]`
  — defaults: branch = current HEAD branch, base = `main` (falls back to
  `master`), server = `$NIT_SERVER` or `http://127.0.0.1:8877`. Prints the
  chain JSON including `web_url` — tell the human where to review. Exit
  ≠ 0 on scan errors; re-running is always safe (idempotent). `--partial`
  marks the chain partial: review can start, merging cannot. Sticky — a
  plain push never clears it. Returns no cursor (see "The cursor").
- `nit ready [--base <ref>] [--branch <name>] [--server <url>]` — same
  defaults; clears the partial flag and refreshes (idempotent).
- `nit wait <cursor> [--oneline]` — consume the chain's `events` stream
  from the 0-based `cursor` and block until something you should act on
  lands, then print `{head, entries, state, …}` (Feedback fields plus
  `head`/`entries`). **No timeout** — call it only when you have nothing
  else to do; it blocks until the reviewer acts (a wake). `--oneline`
  prints a one-line digest per entry instead of full payloads. Returns
  immediately when you are already behind `head`. Survives server restarts:
  the stream reconnects through the outage with backoff (a single stderr
  notice per outage; stdout stays pure JSON).
- `nit log <ranges> [--oneline] [--server <url>]` — print specific log
  entries without touching your cursor: a bare index (`3`), a half-open
  range (`3..6`), an open end (`3..`, `..6`, `..` for all), or several at
  once (concatenated in order, duplicates kept). A reversed/empty range or
  one reaching past the log is an error. For inspecting entries a `wait`
  surfaced that you want the full detail on.
- `nit status` — current Feedback JSON without blocking (no entries, no
  cursor).
- `nit reply <comment-id> [--resolve | --unresolve] -m "text"` — threaded
  reply as the agent; `--resolve` closes the thread (do this for addressed
  comments — the reviewer sees unresolved counts), `--unresolve` reopens
  it, neither leaves it unchanged. Appends a `reply` entry; returns no
  cursor.

## Where the conversation happens

nit is the single source of truth for the review conversation. When you
need something from the reviewer — a clarifying question, a design choice,
a trade-off for them to pick — raise it with `nit reply <comment-id> -m
"…"` on the thread it concerns, leave it **unresolved** so it stays on
their radar, then re-arm `nit wait` and carry on with other work. Do
**not** block on the answer, and do not route the question through some
other channel: your interactive session is the channel only when the user
prompts you there directly. Asking in nit pins the question to the code
it's about, lets the reviewer answer asynchronously, and leaves one
durable record of why the change ended up the way it did.

## What `nit wait` returns

`nit wait` prints the Feedback shape (docs/api.md) plus `head` and
`entries`. Decide on `state`, using `entries` to see exactly what changed:

- `agents_turn` — act now. For every change with status
  `changes_requested` or `commented`: address its `review.message` and
  `comments` (fix every comment by amending the commit it targets, or
  reply/`--resolve` with reasoning). Then `nit push` and wait again.
  `commented` means the reviewer asked questions without blocking —
  reply, don't just wait.
  Exception: on a partial chain (`chain.partial: true`) with **no**
  `changes_requested`/`commented` entries, `agents_turn`
  just means every pushed change is approved — the reviewer is caught up.
  Not an error, nothing to address: keep pushing commits, or `nit ready`
  when the branch is done.
- `approved` — every change approved: rebase onto the base if it
  moved (re-formatting each replayed commit), merge/ff, done. The chain
  leaves the dashboard on the next scan.
- `waiting_for_review` — nothing actionable (it woke on your own
  just-pushed entries); wait again.
- `merged` / `abandoned` — the chain is closed; stop.

Comments in feedback are scoped to each change's **latest review**, plus
any still-unresolved threads from earlier reviews. Each comment is pinned
to the `revision` it was written on; its `line_text` shows the exact line
it was commented on, and `side: "old"` anchors to a line in that
revision's parent tree (a deleted/pre-change line).

Comments with `file: "/COMMIT_MSG"` target the **commit message** (line
numbers are 1-based message lines). Answer them by rewording the commit
(interactive-rebase reword / `git commit --amend`) — keep the
`Change-Id:` trailer. A reword creates a new revision and resets the
change to `pending`, exactly like a code edit.

Never submit a review verdict yourself (`POST /api/changes/*/reviews` is
the human's side). The agent surface is push / ready / wait / log /
status / reply.
