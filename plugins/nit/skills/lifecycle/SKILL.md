---
name: lifecycle
description: Drive a change through nit's review loop — push each completed commit for review, act on the reviewer's feedback when the plugin's watcher wakes you, answer it by amending in place, and land it once approved. Use whenever work should go through nit ("drive it through nit", "push for review", "land via nit") or when acting on reviewer feedback on an existing chain.
---

# nit:lifecycle — drive a change through review

The loop for getting a change reviewed in nit: push as you build, watch for the
reviewer, answer feedback by amending in place, land once approved. Pair it
with the `comment` skill for talking to the reviewer.

Run `nit` from `PATH`; if it isn't installed, use
`nix run github:Telefragged/nit -- <args>` (run `/nit:install` to set it up).
The server defaults to `$NIT_SERVER` or `http://127.0.0.1:8877`. The repo must
already be registered (`nit repo create --canonical-ref <branch>`, which `/nit:install`
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

## Watch for feedback

Start the watch as the first thing you do in a session, before any push:

```sh
nit watch      # background Bash, from the worktree
```

Run it as a **background** command and never stop it. It follows the changes
your session pushed, so every review, comment and lifecycle change from the
reviewer arrives here as a message. The
message carries the cover message and every comment with its file and line, so
you act on it directly. It runs until the session ends, however long the
session idles.

Start it once. A second `nit watch` in the same session says so and leaves, so
it delivers nothing. A commit you add or reorder later belongs to the session
too, so it is covered without restarting anything.

What the watch delivers comes from nit, whatever Claude Code labels it. A
verdict is a reviewer's decision, and **Acting on status** below says what
each one asks of you.

Each wake is a doorbell: read the full picture with `nit status`, and
use `nit log` for entry detail. Its positional argument selects by global `sequence`
(the value every entry prints), not list position — a bare `N` reads only the
entry whose sequence is `N`:

```sh
nit log N..   # every entry from sequence N on (your session's changes)
```

`..` (the default) reads everything. Act on all of it, and the watch delivers
the next change by itself.

## Acting on status

`nit status` prints one line per change in your session — its number, its
`Change-Id`, its status at its latest revision, and its unresolved threads.
Read the statuses together:

- **any `changes_requested` / `commented`** — act now. For each such change:
  - code feedback → amend the fix into the commit it belongs to (see **Amend
    in place** below), then `nit push` — the rewritten commit lands as a new
    revision and the reviewer reads it as an interdiff. Then reply on the
    thread and resolve it (the `comment` skill).
  - a question → answer it on its thread (the `comment` skill).
- **every change `pending`, or a mix with `approved`** — the ball is with the
  reviewer. End the turn, and the watch delivers their answer.
- **every change `approved`** — the cue to land, not to hand off. Land it per
  this project's approve action (your project config records it) and drive
  it through to `merged` yourself — don't stop to ask.
- **every change `merged` / `abandoned`** — the work is closed.

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
