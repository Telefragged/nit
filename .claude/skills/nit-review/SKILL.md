---
name: nit-review
description: Land changes through nit's review loop (push --partial per commit → ready → wait → amend → ff-merge). Use as soon as the first commit on a feature branch is done — review runs alongside the build — on "push for review", or when acting on nit feedback. Exemptions: docs/dev.md.
---

# nit-review — the dogfood loop

This repo reviews itself with nit. Protocol reference: `docs/agent-workflow.md`.
This skill is the operational checklist for driving it from a Claude session.

## When

- The first commit of a piece of work is done on a feature branch and a
  human should review the work before it lands on `main` — review runs
  alongside the rest of the build, not after it.
- The user asks for review, or asks to act on feedback from an existing chain.
- You are spawning agents that build branches (one worktree/branch per
  task): the loop belongs to **each worker, for its own branch, from its
  own worktree** — write it into every worker's instructions. Do not keep
  nit interaction to yourself as a coordinator, batch pushes, or schedule
  them after later phases; until someone pushes, the reviewer sees nothing.

**When not:**

- The change matches a "Review exemptions" entry in `docs/dev.md`, or the
  user opts this change out ("skip nit", "land directly"). Skipping nit
  skips the _review_, not the branch discipline: finish the work on its
  branch/worktree and ff-merge to `main` exactly where the loop's merge
  step would have run.
- The current _commit_ is mid-flight. Push only completed, green commits;
  an incomplete chain is fine — that is what `--partial` marks. Completed
  is not final: a planned cleanup/self-review/verification pass that may
  still amend the commit is no reason to hold the push — post-push amends
  become new revisions by design.

## Preconditions

- CLI (`nit` below): use `nit` from PATH if installed, else
  `nix run 'git+file://<primary-checkout>?ref=main#nit' -- <args>` —
  that builds from `main` (matching the running server, not your possibly
  diverged branch) and never touches the `result` symlink that the user
  or other agents may rely on. Don't run `nix build` for this.
- Server: `curl -fsS http://127.0.0.1:8877/api/health`. If it is down,
  tell the user to start `nit serve` — do not start one yourself unless
  asked (the server and its database belong to the user).
- Every commit: builds green first, one concern, and its own
  `Change-Id: I<40hex>` trailer — required; a missing or duplicated
  trailer (or a pushed `fixup!`/`squash!` commit) fails the scan. **All
  trailers in one block** — a blank line between `Change-Id:` and
  `Co-Authored-By:` splits the block and the trailer is silently lost
  (git last-paragraph rule). Generate:
  `python3 -c 'import secrets; print("I"+secrets.token_hex(20))'`.

## The loop

```sh
# after EVERY completed commit (green, one concern, Change-Id'd):
nit push --partial  # register/refresh the chain as partial (sticky)
# → FIRST push: report web_url to the user now — review starts on
#   commit one, not when the branch is done
# after the LAST commit:
nit ready           # clears partial; the chain can now reach ready_to_merge
nit wait            # blocks; prints Feedback JSON on wake
```

After `nit ready`, run `nit wait` as a background Bash task so the review
wakes the session. Feedback arriving mid-build is handled exactly like the
`agents_turn` branch below, folded into the next incremental push
(`nit status` shows it without blocking). On wake, branch on
`feedback.state`:

- **`agents_turn`** — for each change with `request_changes`/`commented`:
  - code feedback → fix it by amending the commit in place, keeping its
    Change-Id: `git commit --fixup=<commit_sha of the change>`, then
    `GIT_EDITOR=true git rebase --autosquash "$(git merge-base main HEAD)"`
    — squash **before** pushing (pushed `fixup!` commits fail the scan),
    and onto the fork point, not moved main, so interdiffs stay clean.
    Then `nit reply <comment-id> --resolve -m "what you did"`;
  - questions → `nit reply` with the answer (`--resolve` when settled);
  - Then `nit push` (the rewritten commits become new revisions) and wait
    again.
  - On a partial chain, `agents_turn` with none of the above (every pushed
    change approved) is not an error and not feedback — the reviewer is
    caught up. Keep building, or `nit ready` when the branch is done.
- **`ready_to_merge`** — every change approved. Land it (order matters —
  scan must see the merge while the branch ref still exists, so it records
  `merged`, not `abandoned`):
  ```sh
  git rebase main                 # only needed when main moved
  git checkout main && git merge --ff-only <branch>
  nit push --branch <branch>      # scan flags the chain merged
  git branch -d <branch>
  ```
  In a worktree (`.worktrees/*`): rebase there, but never `git checkout
main` — main is checked out elsewhere. Run the merge from the primary
  checkout: `git -C <primary-checkout> merge --ff-only <branch>`; if that
  checkout isn't yours to drive (parallel agents), stop at
  `ready_to_merge` and report to the coordinator.
- **`merged` / `abandoned`** — chain is closed; stop.
- **`waiting_for_review`** — poll timeout; wait again.

Never submit a review verdict yourself (`POST /api/changes/*/reviews` is
the human's side). The agent surface is push / ready / wait / status /
reply.

## Wait pitfalls (learned in production)

- `nit wait` before `nit ready` on a partial chain whose pushed changes are all
  approved returns immediately, forever — all-approved-while-partial is
  `agents_turn` (actionable, "reviewer caught up"). Do not spin: keep
  building; wait only after `nit ready`.
- A comment-only verdict leaves the change `commented` (actionable) until a
  **new revision** lands. If you answered with replies alone — no code
  change — `nit wait` returns immediately, forever. Do not spin: report to
  the user and stop, or wait edge-triggered on the raw endpoint
  (`GET /api/chains/{id}/wait?cursor=<last>&timeout=55`, looping while the
  cursor is unchanged).
- Edge-triggered waiting has a bootstrap race: events landing before your
  first cursor read are invisible. Always check
  `GET /api/chains/{id}/feedback` for unresolved reviewer comments _after_
  taking the cursor and _before_ blocking.
- If a push fails with a Change-Id scan error (missing or duplicate
  trailer, or a `fixup!`/`squash!` commit), fix the commit messages and
  push again — a blank line splitting the trailer block is the usual
  culprit for "missing".
