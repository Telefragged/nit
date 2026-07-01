---
name: lifecycle
description: Drive a change through nit's review loop — push each completed commit for review, watch for the reviewer with a monitor, answer feedback by amending in place, and land it once approved. Use whenever work should go through nit ("drive it through nit", "push for review", "land via nit") or when acting on reviewer feedback on an existing chain.
---

# nit:lifecycle — drive a change through review

The loop for getting a change reviewed in nit: push as you build, watch for the
reviewer, answer feedback by amending in place, land once approved. Pair it
with the `comment` skill for talking to the reviewer.

Run `nit` from `PATH`; if it isn't installed, use
`nix run github:Telefragged/nit -- <args>` (run `/nit:install` to set it up).
The server defaults to `$NIT_SERVER` or `http://127.0.0.1:8877`. The repo must
already be registered (`nit repo create --base <branch>`, which `/nit:install`
does) — a push into an unregistered repo is a 404.

## Push as you build

Pushing is part of finishing a commit, not a later phase. The moment a commit
is done, push it — an unpushed commit is invisible to the reviewer.

```sh
# after EVERY completed commit:
nit push      # registers the commit; the first push starts review
```

Run it from inside the worktree — it resolves the repo and tip from the
checked-out commit. Report the first push so the reviewer knows review has
started. Keep the commits small and don't ration them; the reviewer is never
blocked by more commits.

## Watch for feedback with a monitor

Whenever the chain is open and you have nothing else to do, a watcher must be
running — never end a turn with an open chain and nothing watching it.

Run a parked monitor under the **Monitor tool** (set it persistent), not under
background `Bash`:

```sh
nit log --follow --reviewer-only 0
```

`--follow` streams each new entry as it lands and never exits — so a background
`Bash` task (which only notifies you when a command _exits_) would silently
swallow the stream. The Monitor tool turns each relayed line into a
notification you act on. Run it from the worktree so it resolves the cwd's tip
from HEAD — no id to look up. `--reviewer-only` mutes your own echoes; each
relayed review carries its cover message and every comment with its file and
line, so you act on it directly. `0` streams from the start (resume after a
restart by passing the last seq you saw).

The monitor resolves the tip once, when it starts, so after you stack a new
commit on top, re-run it to pick up the new tip — resuming from the last seq
you consumed (not `0`) so you don't replay what you've already handled.

Each relayed line is a doorbell: read the full picture with `nit status`, and
use `nit log` for entry detail. Its positional argument is a range of log
positions, not a single index — a bare `N` reads only position `N`:

```sh
nit log N..   # all log entries from position N on (resolves the cwd's chain)
```

`..` (the default) reads everything. Act on all of it, then let the monitor
keep streaming.

## Acting on state

`nit status` prints the chain `state`:

- **`agents_turn`** — act now. For each change marked `changes_requested` /
  `commented`:
  - code feedback → amend the fix into the commit it belongs to (see **Amend
    in place** below), then `nit push` — the rewritten commit lands as a new
    revision and the reviewer reads it as an interdiff. Then reply on the
    thread and resolve it (the `comment` skill).
  - a question → answer it on its thread (the `comment` skill).
- **`waiting_for_review`** — the ball is with the reviewer; keep the monitor
  running.
- **`approved`** — the cue to land, not to hand off. Land it per this project's
  approve action (your project config records it) and drive it through to
  `merged` yourself — don't stop to ask.
- **`merged` / `abandoned`** — the chain is closed. Stop the monitor.

Never submit a review verdict yourself — that is the human's side. Your surface
is push / status / log / comment.

## Amend in place

A review fix belongs _in the commit that drew it_ — amend that commit, never
add a separate "address review" commit. The rewrite pushes as a new revision
and the reviewer reads it against the last one.

- **Tip commit** — edit, `git commit --amend`, push.
- **Interior commit** (anything below the tip) — don't tear the stack apart
  with reset or cherry-pick. Stage the fix and let git route it to the right
  commit:

  ```sh
  git commit --fixup <sha>     # <sha> of the commit being fixed
  GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash <base>
  ```

  `<base>` is the branch the chain is stacked on. `-i` runs non-interactively
  because `GIT_SEQUENCE_EDITOR=true` accepts the generated todo unedited.

The **`Change-Id:` trailer must survive the rewrite** — it is what binds the
new revision to the reviewed change. `--amend` and `--fixup` keep it; a reword
that rebuilds the message from scratch (`-m`/`-F`) drops it, the commit hook
mints a fresh one, and the next push _orphans_ the change and restarts its
review. Carry the original `Change-Id:` (and any `Co-Authored-By:`) trailers
across every reword.
