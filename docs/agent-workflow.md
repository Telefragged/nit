# Agent workflow — how coding agents use nit

nit reviews **changes**, not branches. A change is a `Change-Id` scoped to a
repo; it owns an append-only log whose fold is its reviewable state
(docs/data-model.md). Make each commit one reviewable unit (one logical
change, own subject + body). A **chain** is the path of changes from the
canonical base up to a tip commit — never stored, derived at read time by
walking the tip's `parent_sha` back through each revision (docs/api.md
"Chains"). The branch you push is just the tip; each commit on it is a change
the human reviews separately.

This doc is the **agent-driven** push/read/reply loop. Human-operator CLI
conveniences belong in the `clap` help, not here.

After the last commit, `nit wait` blocks on the websocket change stream
(docs/api.md "Events") until the reviewer responds — no polling. `nit status`
and `nit log` stay for one-shot reads.

## Conventions for your commits

- **A `Change-Id: I<unique-token>` trailer on every commit** (any opaque
  string, 40 hex by convention), never reused. It is the change's identity
  across rebases and amends — the same Change-Id reached by two pushes on two
  parents is one change with two patchsets. A commit missing its trailer, or a
  trailer that repeats within one push, rejects the **whole** push (400). Keep
  the trailer block unbroken — a blank line between trailers makes git drop the
  `Change-Id`.
- **Never merge into your branch** (no `git pull` without `--rebase`) — a merge
  commit (or a root commit in the walk) rejects the push. Rebase onto the base.
- **Every commit formatter-clean** (`nix develop -c treefmt`), each rewritten
  commit re-formatted after a rebase (docs/dev.md "Formatting"), so the
  reviewer's interdiff shows your fix, not whitespace.
- **Answer feedback by amending the reviewed commit in place**, keeping its
  Change-Id, then pushing — the moved commit-sha appends a new revision and the
  reviewer sees an interdiff, not "address review comments" commits.
  `fixup!`/`squash!` are fine locally but squash before pushing (the push
  rejects them): `GIT_EDITOR=true git rebase --autosquash <fork-point>`.

A pure rebase — a commit whose patch-id and message are unchanged but whose sha
moved because the base or an earlier change shifted — still appends a revision,
but **does not reset review status**: an approved change stays approved through
it. A reworded commit message _does_ reset the change to `pending` (the message
is reviewable as `/COMMIT_MSG`).

## The loop

**Pushing is part of completing a commit, not a later phase.** Commit, push
`--partial`, build the next. A planned later pass (cleanup, self-review,
verification) never holds a push — post-push amends become new revisions by
design. The only thing that delays a push is the commit not being done.

```sh
repo=$(pwd); branch=$(git branch --show-current)   # both required on push

# after EVERY completed commit (green, formatter-clean, one concern, Change-Id'd):
nit push --partial --repo "$repo" --branch "$branch"   # register/refresh
#   first push creates the change(s) and the chain — report the chain web_url now
nit ready --repo "$repo" --branch "$branch"            # last commit done: clears partial

# then read the chain and act on feedback:
nit status                 # the derived chain digest (state + one line per change)
#   for each change changes_requested/commented: fix by amending the commit it
#     targets (fixup! + autosquash), or answer its thread:
#     nit comment --change-id <Change-Id> --thread <id> [--resolve] -m "…"
nit push --repo "$repo" --branch "$branch"   # amended commits = new revisions
#   re-read until state=approved, then run the project's approve action
```

`nit push`/`nit ready` are the only writers of revisions and the partial flag.
The reviewer's verdicts, the merge/abandon timer, and your own comments all
land in the log independently — so after a push you **re-read** to see them.

### Partial vs ready

`--partial` tracks **work in flight, not review state**. A chain is partial
while a work unit is open (more commits coming, or an in-flight rebase) and
`ready` the moment it completes. Being under review does not make a chain
partial. Both states are reviewable; partial only blocks the `approved` state
(merging), never review. `--partial` is sticky — a plain `nit push` leaves the
flag untouched; `nit ready` is the only thing that clears it.

The partial flag lives on the **tip** change's latest revision (the work
frontier). A shared interior change carrying a stale partial from a sibling
push does not hold an unrelated chain partial.

### Rebasing onto a moved base

`base` configures the repo's **one canonical branch**, recorded on the repo's
first push; every later push must name the same base or it is a 400. To rebase
a chain onto a moved canonical branch, rebase locally first (no server call),
then push — the walk re-forks at the new merge-base and appends a revision to
each change whose sha moved (pure rebases keep their status). A change whose
ancestor has landed shows live with a "newer revision landed elsewhere" note
until you rebase past it.

The push duty is **per branch, owned by whoever builds it**. In multi-agent
setups every worker drives its own `nit push --partial` from its own worktree
the moment its first commit is green; the orchestrator must not centralize,
batch, or phase-gate pushing. An unpushed branch is invisible to the reviewer.

## The per-change cursor

The agent owns a **per-change cursor**: a vector of `change_id → idx`, the
0-based count of each change's log entries it has consumed (an absent key ⇒ 0,
so a newly stacked change replays from the start). `nit push` and `nit comment`
return **no cursor** — an entry that lands between two of your own actions is
caught only because you re-read the change's log run, not because a push told
you about it. Advance a change's slot to its log `head` after you drain it.

- `nit wait` — **block** on the websocket until something past your cursor
  should wake you, then print `{cursor, entries, feedback}` and exit. Call it
  when you have nothing else to do; it derives its watch set from local HEAD,
  rides out a server restart, and applies the wake rule (docs/data-model.md) so
  a comment-less approve that doesn't complete the chain doesn't spin you. Pass
  the printed `cursor` back next call.
- `nit log --follow [--reviewer-only] <cursor>` — a **parked monitor** that
  relays each new entry as it lands (raw, or filtered to reviewer activity with
  `--reviewer-only`). Unlike `nit wait` it never exits — a long-lived watcher.
- `nit status` — the derived **chain digest** for a one-shot read: `state`
  plus, per member, `position change_key status rN Nu subject`.
- `nit log` — the **aggregated chain log**: every member's entries merged and
  sorted by the global `seq`, sliced by position (`3`, `5..9`, `..`).

Two coordinates sit on every entry (docs/api.md): the per-change `idx` (what a
change's own cursor slot advances) and the global `seq` (the aggregated log's
order). `nit wait`/`--follow` own a **vector cursor** (`change_id → idx`) and
subscribe their watch set over one websocket (docs/api.md "Events").

### Reading the chain state

`nit status` prints the derived `state` (docs/api.md "State table"). Branch on
it:

- `agents_turn` — act now. Some member is `changes_requested`/`commented`, or
  the tip is empty, or every member is approved while the chain is still
  `partial`. For each `changes_requested`/`commented` change, address its
  review message and its threads (fix by amending the target commit, or
  reply/`--resolve`), then push. `commented` = questions without blocking;
  reply, don't just ignore it. On a `partial` chain with everything approved,
  `agents_turn` just means the reviewer is caught up — keep pushing, or
  `nit ready` when done.
- `waiting_for_review` — nothing actionable (any member still `pending`); the
  ball is with the reviewer. Re-read later.
- `approved` — every member approved and not `partial`; run the project's
  approve action.
- `has_abandoned` — some member is abandoned (see `nit reopen`).
- `merged` — every member landed; the chain is off the dashboard. Stop.

A change's displayed status is per `(change, revision)` — the latest review at
the patchset the path pins, with `merged`/`abandoned` terminal. Two tips
pinning one change at two patchsets carry independent verdicts; a request in
one chain never overwrites an approve in another.

### Threads

Threads belong to the change and are anchored to the `revision` they were
written on, never ported onto a newer one. A thread on `/COMMIT_MSG` targets
the commit message (1-based lines) — answer by rewording the commit (keep the
`Change-Id`); a reword is a new revision and resets the change to `pending`. A
`side: "old"` anchor names a pre-change/deleted line. `counts.unresolved` on a
path member is scoped to the pinned revision.

Never submit a verdict yourself (`POST /api/changes/*/reviews` is the human's
side). The agent surface is push / ready / status / log / comment / reopen.

## Annotate the choices you make

Made a non-obvious call mid-build (a new dependency, one approach over others, a
workaround)? Leave a thread on the exact lines instead of asking the human or
hoping the diff speaks:

```sh
# a choice the reviewer should weigh in on — leave it OPEN (counts unresolved):
nit comment --change-id <Change-Id> --file src/queue.rs --line 42 --range 42:8-42:30 \
  -m "Bounded channel over unbounded: backpressure matters more than never
      blocking the producer. Happy to flip it."
# a settled decision that just needs recording — open it --resolve'd:
nit comment --change-id <Change-Id> --file Cargo.toml --line 14 --resolve \
  -m "Added serde — alternatives pull it in transitively anyway."
```

Anchor as tightly as you can so the note pins to the code. An agent-opened
thread is an ordinary thread — the reviewer replies and resolves it like any
other. Make the call, annotate it, keep going.

## Where the conversation happens

nit is the single source of truth for the review conversation. Need something
back from the reviewer (a question, a trade-off to pick)? Raise it in nit and
leave it **unresolved**: reply on the relevant thread, or open a new one with
`nit comment`. Then carry on — don't block on the answer, and don't route it
elsewhere. Your interactive session is the channel only when the user prompts
you there directly.

## Abandoned changes

The background lifecycle timer (docs/data-model.md "Lifecycle") marks a change
`abandoned` when its latest revision is unreachable from any branch ref for the
abandonment window — typically because you deleted or force-moved its branch
past it. Abandonment is **change-wide** and terminal until cleared.

A push that would add a revision to an abandoned change is a **409** — reopen
it explicitly first:

```sh
nit reopen --change-id <Change-Id>     # or --change <numeric id>
nit push --repo "$repo" --branch "$branch"   # the new revision folds it to pending
```

Reopen clears `abandoned` back to the change's retained verdict status; the next
push's revision folds it to `pending`. (The timer also writes `merged` when a
change's patch lands on the canonical branch — a push never observes that
itself.)

## Command reference

- `nit push --repo <path> --branch <name> [--partial] [--base <ref>] [--server <url>]`
  — `--repo` (a worktree path) and `--branch` are required: the repo identity is
  the git-common-dir, the branch is the tip; no cwd fallback, or a stray push
  forks against the wrong repo. Defaults: base `main`, server `$NIT_SERVER` or
  `http://127.0.0.1:8877`. Prints the `PushResult` (the tip change + the derived
  chain with its `web_url`). Idempotent — a re-push where nothing moved records
  nothing and succeeds (200); a structural fault is a 400, a revision to an
  abandoned change a 409. `--partial` is sticky (a plain push never clears it).
  No cursor returned.
- `nit ready --repo <path> --branch <name> [--base <ref>] [--server <url>]` —
  clears the partial flag and refreshes. Idempotent.
- `nit status [--chain <tip-change-id>] [--oneline] [--server <url>]` — the
  derived `Chain` for the cwd's tip (or `--chain`), no blocking. `--oneline`
  prints a `state=` header plus one line per member
  (`position change_key status rN Nu subject`).
- `nit log [<ranges>…] [--chain <tip-change-id>] [--oneline] [--server <url>]` —
  the aggregated chain log (members merged, sorted by global `seq`), sliced by
  **position**: `3`, `5..9`, `5..`, `..9`, `..` (all, the default), several at
  once. Positions clamp to the log length. `--chain` reads any chain by its tip
  change id (no cwd needed). Read-only; advances no cursor.
- `nit comment (--change-id <Change-Id> | --change <id>) [--thread <id>] [anchor] [--resolve | --unresolve] [-m "text"]`
  — comment as the agent. `--change-id` is the full trailer (a human can use
  `--change <numeric id>`). Without `--thread`: opens a thread, anchored
  `--file <path> --line <n> [--side new|old] [--range S-E] [--revision <n>]` (or
  change-level with no anchor); `--range` is `START-END`, each endpoint
  `line:char`. With `--thread`: replies to that thread (anchor flags ignored).
  `--resolve`/`--unresolve` set the thread state; a `--thread` reply may carry
  no `-m` when it only resolves/reopens. Appends a `comment`; no cursor.
- `nit reopen (--change-id <Change-Id> | --change <id>) [--server <url>]` —
  clear an abandoned change back to its retained status, so a new revision may
  be pushed. A no-op on a non-abandoned change.

`nit status`/`nit log` resolve the cwd's tip change from local HEAD (the chain
whose tip commit-sha equals HEAD), so run them from the worktree;
`nit comment`/`nit reopen` target a change directly. The human's review verbs
(drafts, reviews) are the web UI and the reviewer endpoints (docs/api.md), not
the agent CLI.
