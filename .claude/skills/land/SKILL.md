---
name: land
description: The approve action for the nit repo — land an approved chain onto main with scripts/land.sh: rebase onto main if it moved, run `nix flake check` on every commit, fast-forward merge, then confirm the chain flips to `merged`. Use whenever a chain you built reaches `approved` and needs landing/merging to main — `approved` is the cue to land, not to hand off. Never run it on a chain that isn't `approved`, and never submit the review verdict yourself.
---

# land — the approve action for this repo

nit derives `approved` (every live change approved) but leaves landing to each
project. Here it's a fast-forward-only merge to `main` — no merge commits
(golden rule 2), every commit green under `nix flake check` — automated by
`scripts/land.sh`. The agent that built the chain **drives it all the way to
`merged`**: `approved` is the cue to land, never to hand off.

## Land it

1. **Check the chain is `approved`.** `nit status` must report
   `state: approved`. Any other state — stop; landing a chain the human hasn't
   approved is never your call.

2. **Run the script** from inside the chain's worktree (`.worktrees/<slug>`):

   ```sh
   nix develop -c scripts/land.sh
   ```

   The happy path is quiet — one line per step:

   ```
   branch rebased        # only printed if main had moved
   flake check passed
   branch merged into main
   ```

3. **Confirm `merged`.** The script fast-forwards `main`; a background timer
   (~5s) observes the move and appends the `merged` entry — the merge isn't
   done until that lands. Poll `nit status` until `state: merged`. If it
   hasn't flipped after ~15s, the merge didn't take — investigate, don't
   assume done.

4. **Clean up.** land.sh ran from inside the worktree, so step out to the
   primary checkout first — git won't remove the worktree you're standing in:

   ```sh
   cd "$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
   git worktree remove .worktrees/<slug>
   git branch -d track/<slug>    # -d, not -D: it refuses unless merged
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
