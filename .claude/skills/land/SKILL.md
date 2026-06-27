---
name: land
description: The approve action for the nit repo — land an approved chain onto main with scripts/land.sh (rebase onto main if it moved, run `nix flake check` on every commit, fast-forward merge). Use once `nit status` reports the chain `approved`. Never run it on an unapproved chain, and never submit the review verdict yourself.
---

# land — the approve action for this repo

nit derives `approved` (every live change approved) but leaves landing to each
project. Here it's a fast-forward-only merge to `main` (no merge commits —
golden rule 2) with every commit green under `nix flake check`, automated by
`scripts/land.sh`. The agent that built the chain **drives it all the way to
`merged`**: reaching `approved` is the cue to land, never to hand off.

## Run it

From inside the chain's worktree (`.worktrees/<slug>`), once `nit status` shows
`state: approved`:

```sh
nix develop -c scripts/land.sh
```

The happy path is quiet — one line per step:

```
branch rebased        # only printed if main had moved
flake check passed
branch merged into main
```

Then clean up, and let the lifecycle timer flip the chain to `merged` (poll
`nit status`):

```sh
git worktree remove .worktrees/<slug> && git branch -d track/<slug>
```

## When it stops

The script exits non-zero and prints git's output plus what to do. It covers
the no-conflict case end to end; the failure modes hand you the repo to finish
by hand:

- **Merge conflict** (rebasing onto a moved main) — resolve it,
  `git rebase --continue`, then `nit push`. Resolving conflicts rewrites the
  patch-id the merge timer matches on, so the new revision has to be recorded
  before landing. Re-run the script.
- **`nix flake check` failed on a commit** — you're left on that commit. Fix
  it, `git rebase --continue`, then re-run. A fix that changes the commit's
  content is new work the reviewer hasn't seen: `nit push` it (expect another
  review pass) rather than landing it silently.
- **`main` moved during checks** — another chain landed first and the
  fast-forward is refused. Just re-run the script; it rebases onto the new main
  and retries.

Approval is the human's side — never submit a verdict yourself. Your surface is
push / status / land.
