---
name: nit-review
description: Land changes through nit's review loop (push branch → wait → fixup → ff-merge). Use when work is ready for human review, on "push for review", or when acting on nit feedback. Exemptions: docs/dev.md.
---

# nit-review — the dogfood loop

This repo reviews itself with nit. Protocol reference: `docs/agent-workflow.md`.
This skill is the operational checklist for driving it from a Claude session.

## When

- A unit of work is complete on a feature branch and a human should review
  it before it lands on `main`.
- The user asks for review, or asks to act on feedback from an existing chain.

**When not:**
- The change matches a "Review exemptions" entry in `docs/dev.md`, or the
  user opts this change out ("skip nit", "land directly"). Skipping nit
  skips the *review*, not the branch discipline: finish the work on its
  branch/worktree and ff-merge to `main` exactly where the loop's merge
  step would have run.
- Work is mid-flight. Push only coherent, green chains.

## Preconditions

- CLI (`nit` below): use `nit` from PATH if installed, else
  `nix run 'git+file://<primary-checkout>?ref=main#nit' -- <args>` —
  that builds from `main` (matching the running server, not your possibly
  diverged branch) and never touches the `result` symlink that the user
  or other agents may rely on. Don't run `nix build` for this.
- Server: `curl -fsS http://127.0.0.1:8877/api/health`. If it is down,
  tell the user to start `nit serve` — do not start one yourself unless
  asked (the server and its database belong to the user).
- Every commit: builds green first, one concern, and a `Change-Id: I<40hex>`
  trailer. **All trailers in one block** — a blank line between `Change-Id:`
  and `Co-Authored-By:` splits the block and the trailer is silently lost
  (git last-paragraph rule). Generate:
  `python3 -c 'import secrets; print("I"+secrets.token_hex(20))'`.

## The loop

```sh
nit push            # register/refresh current branch
# → report web_url to the user, then wait in a background task:
nit wait            # blocks; prints Feedback JSON on wake
```

Run `nit wait` as a background Bash task so the review wakes the session.
On wake, branch on `feedback.state`:

- **`agents_turn`** — for each change with `request_changes`/`commented`:
  - code feedback → fix it, `git commit --fixup=<commit_sha of the change>`,
    then `nit reply <comment-id> --resolve -m "what you did"`;
  - questions → `nit reply` with the answer (`--resolve` when settled);
  - `needs_rebase: true` → a fixup conflicted; restructure the branch
    (squash manually), push again before waiting.
  - Then `nit push` (fixups fold into new revisions) and wait again.
- **`ready_to_merge`** — every change approved. Land it (order matters —
  scan must see the merge while the branch ref still exists, so it records
  `merged`, not `abandoned`):
  ```sh
  GIT_EDITOR=true git rebase --autosquash main
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
the human's side). The agent surface is push / wait / status / reply.

## Wait pitfalls (learned in production)

- A comment-only verdict leaves the change `commented` (actionable) until a
  **new revision** lands. If you answered with replies alone — no code
  change — `nit wait` returns immediately, forever. Do not spin: report to
  the user and stop, or wait edge-triggered on the raw endpoint
  (`GET /api/chains/{id}/wait?cursor=<last>&timeout=55`, looping while the
  cursor is unchanged).
- Edge-triggered waiting has a bootstrap race: events landing before your
  first cursor read are invisible. Always check
  `GET /api/chains/{id}/feedback` for unresolved reviewer comments *after*
  taking the cursor and *before* blocking.
- If a push reports `scan_warnings` or a change you didn't expect (its
  `change_key` is a commit sha, not your `I…` token), your Change-Id
  trailer didn't parse — fix the commit message before review starts.
