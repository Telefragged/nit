# Agent workflow — how coding agents use nit

nit reviews **commits**, not branches. Make each commit one reviewable unit
(one logical change, own subject + body). The branch you register is the
"chain"; each commit on it is a change the human reviews separately.

This doc is the **agent-driven** push/wait/reply loop. Human-operator CLI
conveniences belong in the `clap` help, not here.

## Conventions for your commits

- **A `Change-Id: I<unique-token>` trailer on every commit** (any opaque
  string, 40 hex by convention), never reused. It is the change's identity
  across rebases and amends; a missing or duplicate trailer fails the scan
  (`last_scan_error`). Keep the trailer block unbroken — a blank line
  between trailers makes git drop the `Change-Id`.
- **Never merge into your branch** (no `git pull` without `--rebase`) — a
  merge commit fails the scan. Rebase onto the base.
- **Every commit formatter-clean** (`nix develop -c treefmt`), each
  rewritten commit re-formatted after a rebase (docs/dev.md "Formatting"),
  so the reviewer's interdiff shows your fix, not whitespace.
- **Answer feedback by amending the reviewed commit in place**, keeping its
  Change-Id, then pushing — nit appends a new revision and the reviewer
  sees an interdiff, not "address review comments" commits. `fixup!`/
  `squash!` are fine locally but squash before pushing (the scan rejects
  them): `GIT_EDITOR=true git rebase --autosquash <fork-point>`.

## The cursor

nit's state is an append-only log per chain (docs/data-model.md). You drive
review with a **0-based cursor** — the count of entries you've consumed,
starting at `0`. `nit wait <cursor>` returns `{head, entries, state, …}`
where `entries` is `[cursor, head)`; set your cursor to `head`. Two rules
keep it lossless:

1. **Advance the cursor only from a `wait`/`log` `head`**, never from
   `push`/`comment` (they return no index). A reviewer comment landing
   between two of your pushes is only caught if you take the whole
   contiguous run from `wait`.
2. **Keep waiting until it blocks.** Right after a push/reply, `wait`
   returns immediately with your own just-appended entries; process,
   advance, wait again until it actually blocks — then you're parked.

Skim with `--oneline`; inspect entries without moving the cursor via
`nit log <ranges>` (`3`, `3..6`, half-open). Entry `kind`s
(docs/data-model.md): `review` (a reviewer verdict — act on it),
`chain_closed` (merged/abandoned — stop), and your own (`revisions`,
`comment`, `partial`).

## The loop

**Pushing is part of completing a commit, not a later phase.** Commit, push
`--partial`, build the next. A planned later pass (cleanup, self-review,
verification) never holds a push — post-push amends become new revisions by
design. The only thing that delays a push is the commit not being done.

```sh
cursor=0
repo=$(pwd); branch=$(git branch --show-current)   # both required on push
# after EVERY completed commit (green, formatter-clean, one concern, Change-Id'd):
nit push --partial --repo "$repo" --branch "$branch"   # register/refresh
#   first push creates the chain — report its web_url to the human now
nit ready --repo "$repo" --branch "$branch"            # last commit done: clears partial

# then drive the cursor loop until the chain closes:
resp=$(nit wait $cursor); cursor=<resp.head>
#   for each `review` entry: fix by amending the commit it targets
#     (fixup! + autosquash), or answer its thread:
#     nit comment --change-id <Change-Id> --thread <id> [--resolve] -m "…"
nit push --repo "$repo" --branch "$branch"   # rewritten commits = new revisions
#   loop wait→advance until state=approved, then run the approve action
nit push --repo "$repo" --branch "$branch"   # next scan appends chain_closed{merged}
```

**A watcher is mandatory until the chain closes.** The instant `nit ready`
returns (or you push the last revision), a `nit wait` or `nit log --follow`
must be running as a background task and stay up until `approved`/`merged`/
`abandoned`. A ready/pushed chain with nothing watching it is a dropped
review, as broken as an unpushed commit. With `nit wait`, re-arm after every
push/reply. The turn ends when the chain closes, not at `nit ready`.

The push duty is **per branch, owned by whoever builds it**. In multi-agent
setups every worker drives its own `nit push --partial` from its own
worktree the moment its first commit is green; the orchestrator must not
centralize, batch, or phase-gate pushing. An unpushed branch is invisible.

### Partial vs ready

`--partial` tracks **work in flight, not review state**. A chain is partial
while a work unit is open (more commits coming, or an in-flight rebase) and
`ready` the moment it completes. Being under review does not make a chain
partial. Both states are reviewable; partial only blocks merging.

**Never scan (`push`/`ready`) on a stale base** — one `main` has moved past.
The branch then diverges at the old ancestor, and the scan walks the
divergent old-`main` commits in as permanent `orphaned` entries. To rebase a
`ready` chain onto a moved `main`: rebase first (local, no server call),
then `nit ready` once — not `nit push --partial` beforehand.

### Following the log instead of waiting

`nit wait` blocks for one wake. A harness with a **cooperative monitor**
that relays a process's output line-by-line (e.g. Claude Code) can follow
the log instead:

```sh
nit log --follow --oneline --reviewer-only <cursor>   # background monitor
```

- **`--oneline`**: one parseable line per entry, not token-heavy JSON.
- The monitor must wake you **on each relayed line** — `nit log --follow`
  never exits, so a "notify on exit" mechanism sits silent forever. (The
  `nit-review` skill names the concrete tool for this repo.)
- **`--reviewer-only`** mutes your own echoes (`revisions`/`comment`/
  `partial`) and applies `nit wait`'s wake rule: wakes on the reviewer and
  on chain closure, holds back a comment-less non-completing approve. Each
  relayed line is a **doorbell** — re-read the gap with `nit log <cursor>..`
  from the index you last consumed (not the printed idx), act on all of it,
  then advance.
- A bare `nit log --follow` relays every entry raw (no wake rule); you
  triage. It advances no cursor — track the `head` of your last read, resume
  with `nit log --follow <cursor>..`.

The one thing `--reviewer-only` can't surface is a rescan that reopens a
closed chain. So keep the standing rule: watch until the chain closes, and
re-read the full log if you've been idle.

### Command reference

- `nit push --repo <path> --branch <name> [--partial] [--base <ref>] [--server <url>]`
  — `--repo`/`--branch` required (the chain's identity; no cwd fallback, or
  a stray push forks a duplicate). Defaults: base `main`, server
  `$NIT_SERVER` or `http://127.0.0.1:8877`. Prints the chain JSON with
  `web_url`. Idempotent; exit ≠ 0 on scan error. `--partial` is sticky (a
  plain push never clears it). No cursor returned.
- `nit ready --repo <path> --branch <name> [--base <ref>] [--server <url>]`
  — clears the partial flag and refreshes. Idempotent.
- `nit wait <cursor> [--oneline]` — block on the `events` stream from the
  0-based cursor until something actionable lands, then print
  `{head, entries, state, …}`. **No timeout** — call it only when idle.
  Returns immediately if behind `head`. Rides out server restarts.
- `nit log <ranges> [--oneline] [--chain <id>] [--server <url>]` — print
  entries without moving the cursor: `3`, `3..6`, `3..`, `..6`, `..`, or
  several at once. `--chain <id>` reads any chain (no git repo needed).
- `nit log --follow <cursor> [--oneline] [--reviewer-only] [--chain <id>] [--server <url>]`
  — live tail for monitors (above). `<cursor>` is one open form (`0`, `5..`,
  `..`). Rides out restarts; runs until stopped.
- `nit status [--oneline]` — current Feedback JSON, no blocking. `--oneline`
  prints a `state=` header plus one line per change
  (`position change_key status rN Nu subject`).
- `nit comment --change-id <Change-Id> [--thread <id>] [anchor] [--resolve | --unresolve] -m "text"`
  — comment as the agent (cwd resolves the chain). Pass the full
  `--change-id` trailer (a human can use `--change <numeric id>` instead).
  No `--thread`: opens a thread, anchored `--file <path> --line <n>
[--side new|old] [--range S-E] [--revision <n>]` (or change-level with no
  anchor). With `--thread`: replies. `--resolve`/`--unresolve` set the
  thread state. Appends a `comment`; no cursor.

## Annotate the choices you make

Made a non-obvious call mid-build (a new dependency, one approach over
others, a workaround)? Leave a thread on the exact lines instead of asking
the human or hoping the diff speaks:

```sh
# a choice the reviewer should weigh in on — leave it OPEN (counts unresolved):
nit comment --change-id <Change-Id> --file src/queue.rs --line 42 --range 42:8-42:30 \
  -m "Bounded channel over unbounded: backpressure matters more than never
      blocking the producer. Happy to flip it."
# a settled decision that just needs recording — open it --resolve'd:
nit comment --change-id <Change-Id> --file Cargo.toml --line 14 --resolve \
  -m "Added serde — alternatives pull it in transitively anyway."
```

Anchor as tightly as you can so the note pins to the code. Make the call,
annotate it, keep going.

## Where the conversation happens

nit is the single source of truth for the review conversation. Need
something back from the reviewer (a question, a trade-off to pick)? Raise it
in nit and leave it **unresolved**: reply on the relevant thread, or open a
new one with `nit comment`. Then re-arm `nit wait` and carry on — don't
block on the answer, and don't route it elsewhere. Your interactive session
is the channel only when the user prompts you there directly.

## What `nit wait` returns

`nit wait` prints the Feedback shape (docs/api.md) plus `head`/`entries`.
Branch on `state`:

- `agents_turn` — act now. For each change `changes_requested`/`commented`:
  address its `review.message` and `threads` (fix by amending the target
  commit, or reply/`--resolve`), then `nit push` and wait again.
  `commented` = questions without blocking; reply, don't just wait.
  Exception: on a `partial` chain with no `changes_requested`/`commented`,
  `agents_turn` just means every pushed change is approved (reviewer caught
  up) — keep pushing, or `nit ready` when done.
- `approved` — every change approved; run the project's approve action.
- `waiting_for_review` — nothing actionable (woke on your own entries);
  wait again.
- `merged` / `abandoned` — chain closed; stop.

Threads are scoped to each change's latest review plus still-unresolved ones
from earlier. Each is pinned to the `revision` it was written on;
`line_text` is the anchored line, `side: "old"` a pre-change/deleted line. A
thread on `/COMMIT_MSG` targets the commit message (1-based lines) — answer
by rewording the commit (keep the `Change-Id`); a reword is a new revision
and resets the change to `pending`.

Never submit a verdict yourself (`POST /api/changes/*/reviews` is the
human's side). The agent surface is push / ready / wait / log / status /
comment.
