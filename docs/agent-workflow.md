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

After the last commit, `nit log --wait` blocks on the websocket change stream
(docs/api.md "Events") until the reviewer responds — no polling. `nit status`
and `nit log` stay for one-shot reads. Every command prints concise text for
you to act on, not JSON.

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

**Pushing is part of completing a commit, not a later phase.** Commit, push,
build the next. A planned later pass (cleanup, self-review, verification) never
holds a push — post-push amends become new revisions by design. The only thing
that delays a push is the commit not being done.

```sh
nit repo create --base <branch>   # once per repo: register it, pinning the canonical
#   branch; a push into an unregistered repo is a 404
# after EVERY completed commit (green, formatter-clean, one concern, Change-Id'd):
nit push                   # push the checked-out commit; base comes from the repo
#   first push creates the change(s) and the chain — review starts here, on commit one
#   push prints the whole chain digest (state + one line per change), so you see
#   what you pushed without a second command

# then act on feedback:
nit status                 # re-read the chain digest (state + one line per change)
#   for each change changes_requested/commented: fix by amending the commit it
#     targets (fixup! + autosquash), or answer its thread:
#     nit comment --change-id <Change-Id> --thread <id> [--resolve] -m "…"
nit push                   # amended commits = new revisions
#   re-read until state=approved, then run the project's approve action
```

`nit push` is the only writer of revisions. The reviewer's verdicts and
abandon/reopen, the merge timer, and your own comments all land in the log
independently — so after a push you **re-read** to see them.

### Rebasing onto a moved base

The repo's **one canonical branch** is set at `nit repo create` (`--base`);
push neither takes nor configures a base. To rebase a chain onto a moved canonical branch (the
same base, advanced), rebase locally first (no server call),
then push — the walk re-forks at the new merge-base and appends a revision to
each change whose sha moved (pure rebases keep their status). A change whose
ancestor has landed shows live with a "newer revision landed elsewhere" note
until you rebase past it.

The push duty is **per branch, owned by whoever builds it**. In multi-agent
setups every worker drives its own `nit push` from its own worktree
the moment its first commit is green; the orchestrator must not centralize,
batch, or phase-gate pushing. An unpushed branch is invisible to the reviewer.

## The cursor

The agent owns one cursor: the highest global `seq` it has consumed of the
aggregated chain log. `nit push` and `nit comment` return **no cursor** — an
entry that lands between two of your own actions is caught only because you
re-read the log, not because a push told you about it.

- `nit log --wait <cursor>` — **block** on the websocket until entries land
  past the cursor, then print the chain digest (its header carries the new
  `cursor=`) and those entries, and exit. Call it when you have nothing else
  to do; it derives its watch set from local HEAD, rides out a server
  restart, and wakes on any new entry past the cursor (docs/data-model.md
  "Wake rule"). Pass the printed `cursor` back next call.
- `nit log --follow <cursor>` — a **parked monitor** that relays each new entry
  as it lands. Unlike `--wait` it never exits — a long-lived watcher.
- `--reviewer-only` — a filter for any of these reads: keep only the reviewer's
  activity, dropping the agent's own `revision`/`comment` echoes and the
  automatic `merged`. On `--wait` it blocks until reviewer activity lands.

### Reading the chain state

`nit status` prints the derived `state` (docs/api.md "State table"). Branch on
it:

- `agents_turn` — act now. Some member is `changes_requested`/`commented`, or
  the tip is empty. For each `changes_requested`/`commented` change, address its
  review message and its threads (fix by amending the target commit, or
  reply/`--resolve`), then push. `commented` = questions without blocking;
  reply, don't just ignore it.
- `waiting_for_review` — nothing actionable (any member still `pending`); the
  ball is with the reviewer. Re-read later.
- `approved` — every member approved; run the project's approve action.
- `merged` — every member landed; the chain is off the dashboard. Stop.

There is **no abandoned chain state** — abandonment is per-change. A member
shown `abandoned` (a reviewer or you marked it dead via `nit abandon`) is
dropped from the chain's derived state; decide whether to drop the change
(rebase off it) or `nit reopen` it.

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
side). The agent surface is push / status / log / comment / abandon / reopen.

## Annotate the choices you make

Made a non-obvious call mid-build (a new dependency, one approach over others, a
workaround)? Leave a thread on the exact lines instead of asking the human or
hoping the diff speaks:

```sh
# a choice the reviewer should weigh in on — leave it OPEN (counts unresolved):
nit comment --change-id <Change-Id> --file src/queue.rs --range 42:8-42:30 \
  -m "Bounded channel over unbounded: backpressure matters more than never
      blocking the producer. Happy to flip it."
# a settled decision that just needs recording — open it --resolve'd:
nit comment --change-id <Change-Id> --file Cargo.toml --line 14 --resolve \
  -m "Added serde — alternatives pull it in transitively anyway."
```

Anchor as tightly as you can so the note pins to the code. Write the body as
markdown — when you quote code, fence it with a language tag (` ```rust `)
and it renders with the diff's syntax highlighting, so the quote reads like
a reference. For anything beyond a one-liner, prefer `-F -` with a quoted
heredoc over `-m` — no shell escaping to get wrong. An agent-opened thread
is an ordinary thread — the reviewer replies and resolves it like any
other. Make the call, annotate it, keep going.

## Where the conversation happens

nit is the single source of truth for the review conversation. Need something
back from the reviewer (a question, a trade-off to pick)? Raise it in nit and
leave it **unresolved**: reply on the relevant thread, or open a new one with
`nit comment`. Then carry on — don't block on the answer, and don't route it
elsewhere. Your interactive session is the channel only when the user prompts
you there directly.

## Abandoned changes

A change is `abandoned` only when a reviewer or agent **explicitly** marks it
dead — `nit abandon` (or the reviewer's abandon decision in the UI). The
lifecycle timer never abandons: deleting or force-moving a branch leaves its
changes live, not abandoned. Abandonment is **change-wide** and terminal until
cleared.

A push that would add a revision to an abandoned change is a **409** — reopen
it explicitly first:

```sh
nit reopen --change-id <Change-Id>     # or --change <numeric id>
nit push                               # the new revision folds it to pending
```

Reopen clears `abandoned` back to the change's retained verdict status; the next
push's revision folds it to `pending`. (The timer's one job is `merged` — it
writes that when a change's patch lands on the canonical branch; a push never
observes that itself.)

## Command reference

- `nit --version` — print the client and server builds (`client <ver>` /
  `server <ver>`, each `<semver>[+<sha>[.dirty]]`); exits non-zero if the server
  (`$NIT_SERVER` or the default) is unreachable, printing `server unreachable`.
  The canonical check that nit is installed and the server is up — use it in
  place of any ad-hoc `/api/health` probe.
- `nit repo create --base <ref> [--server <url>]` — register the cwd's repo
  (once per repo), pinning its canonical base ref: `--base` is required and
  must resolve to a commit — any git ref, e.g. `origin/main` (400 otherwise);
  nit never guesses it. 409 if the repo is already registered. Prints the
  registered repo line. A `nit push` into an unregistered repo is a 404.
- `nit push [<commit>] [--server <url>]` — push the
  cwd's checked-out commit (HEAD — a detached HEAD or tag included), or an
  explicit `<commit>` (any rev). The repo is the cwd's git-common-dir and must
  already be registered (`nit repo create`); the canonical branch is the repo's
  stored one. Server defaults to `$NIT_SERVER` or
  `http://127.0.0.1:8877`. Prints the resulting chain digest — every change the
  push registered, so no follow-up read. Idempotent — a re-push where nothing
  moved records nothing and succeeds (200); an unregistered repo is a 404, a
  structural fault a 400, a revision to an abandoned change or an already-merged
  tip a 409. No cursor returned.
- `nit status [--chain <tip-change-id>] [--server <url>]` — the derived chain
  digest for the cwd's tip (or `--chain`), no blocking: a `state=` header plus
  one line per member (`position change_key status rN Nu subject`).
- `nit log [<ranges>…] [--chain <tip-change-id>] [--oneline] [--follow | --wait] [--reviewer-only] [--server <url>]`
  — the aggregated chain log (members merged, sorted by global `seq`). One-shot,
  it selects by global **`seq`**: `3`, `5..9`, `5..`, `..9`, `..` (all, the
  default), several at once; a range may span seqs this chain doesn't hold
  (they belong to other changes) and those just match nothing. Each entry
  renders its own payload as text (a review shows its cover message and one
  comment per thread, led by the thread id); `--oneline` is the opt-in terse
  digest instead. `--follow <cursor>` parks as a monitor relaying each new
  entry; `--wait <cursor>` blocks until entries land past the `seq` cursor,
  prints the chain digest and those entries, and exits. `--reviewer-only` is a
  filter for any mode: keep only reviewer activity, dropping your own echoes
  and the auto-merge (on `--wait`, block until reviewer activity lands).
  `--chain` reads any chain by its tip change id (no cwd needed). Read-only;
  advances no cursor.
- `nit comment (--change-id <Change-Id> | --change <id>) [--thread <id>] [anchor] [--resolve | --unresolve] [-m "text"]`
  — comment as the agent. `--change-id` is the full trailer (a human can use
  `--change <numeric id>`). Without `--thread`: opens a thread, anchored
  `--file <path> (--line <n> | --range S-E) [--side new|old] [--revision <n>]`
  (or change-level with no anchor); `--range` is `START-END`, each endpoint
  `line:char`, and anchors the thread under END's line. With `--thread`: replies to that thread (anchor flags ignored).
  `--resolve`/`--unresolve` set the thread state; a `--thread` reply may carry
  no `-m` when it only resolves/reopens. The body is markdown; `-F <path>`
  reads it from a file (`-` for stdin — prefer `-F -` with a heredoc for
  multi-line bodies). Appends a `comment`; no cursor.
- `nit abandon (--change-id <Change-Id> | --change <id>) [-m "reason"] [--server <url>]`
  — mark a change dead (terminal until reopened), optionally recording a
  reason. A push that would revise an abandoned change is a 409.
- `nit reopen (--change-id <Change-Id> | --change <id>) [--server <url>]` —
  clear an abandoned change back to its retained status, so a new revision may
  be pushed. A no-op on a non-abandoned change.

`nit status`/`nit log` resolve the cwd's tip change from local HEAD (the chain
whose tip commit-sha equals HEAD), so run them from the worktree;
`nit comment`/`nit abandon`/`nit reopen` target a change directly. The human's review verbs
(drafts, reviews) are the web UI and the reviewer endpoints (docs/api.md), not
the agent CLI.
