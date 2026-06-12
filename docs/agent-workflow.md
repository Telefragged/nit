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
- Answer review feedback by **amending the reviewed commit in place**,
  keeping its Change-Id, then pushing the rewritten branch. nit tracks
  the rewrite as a new revision of the same change; the reviewer sees
  what you changed (interdiff), not a pile of "address review comments"
  commits. `fixup!`/`squash!` commits are fine as a local staging step,
  but squash them before pushing — the scan rejects them (git ≥ 2.44):
  `GIT_EDITOR=true git rebase --autosquash <fork-point>`
  (fallback: `GIT_SEQUENCE_EDITOR=: GIT_EDITOR=true git rebase -i --autosquash <fork-point>`).
- Rewrite onto your chain's **fork point**
  (`$(git merge-base <base> HEAD)`), not the moved base: rebasing
  mid-review drags unrelated base drift into every interdiff. Rebase onto
  the base itself only when you actually need to — landing, or a real
  conflict.

## The loop

```sh
# while building — after EVERY completed commit (green, one concern,
#   Change-Id'd), not once at the end:
nit push --partial            # register/refresh the chain as partial
#   the FIRST push creates the chain — report web_url to the human now;
#   review starts on commit one.
nit ready                     # last commit done: clears partial, refreshes —
                              #   the chain can now reach ready_to_merge
nit wait                      # block until the reviewer acts; prints JSON
# read feedback; for each comment: fix → amend the commit it targets
#   (local fixup! + autosquash onto the fork point, or interactive
#   rebase), or answer with: nit reply <comment-id> [--resolve] -m "…"
nit push                      # the rewritten commits become new revisions
nit wait                      # …repeat until state=ready_to_merge
# then: rebase onto <base> if it moved; merge/ff the branch
nit push                      # optional: next scan marks the chain merged
```

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
  marks the chain partial: review can start, merging cannot. Sticky — a plain
  push never clears it. Feedback can land mid-build: each push response
  carries the change statuses, and `nit status` shows the full Feedback
  JSON without blocking — handle it as normal `agents_turn` work
  (amends/replies below), folded into the next incremental push.
- `nit ready [--base <ref>] [--branch <name>] [--server <url>]` — same
  defaults; clears the partial flag and refreshes (idempotent).
- `nit wait [--timeout <secs>]` — returns immediately when the state is
  actionable, else long-polls (internally re-polling until `--timeout`,
  default forever). Exit 0 with the Feedback JSON on stdout. Survives
  server restarts: transport failures are retried with backoff (a single
  stderr notice per outage; stdout stays pure JSON). With `--timeout`,
  expiry while the server is unreachable exits non-zero instead of
  printing a stale snapshot.
- `nit status` — current Feedback JSON without blocking.
- `nit reply <comment-id> [--resolve] -m "text"` — threaded reply as the
  agent; `--resolve` closes the thread (do this for addressed comments —
  the reviewer sees unresolved counts).

## Feedback JSON (printed by `nit wait` / `nit status`)

Shape: `Feedback` in docs/api.md. Decide on `state`:

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
- `ready_to_merge` — every change approved: rebase onto the base if it
  moved, merge/ff, done. The chain leaves the dashboard on the next scan.
- `waiting_for_review` — nothing actionable (the poll timed out); wait
  again.
- `merged` / `abandoned` — the chain is closed; stop.

Comments in feedback are scoped to each change's **latest review**, plus
any still-unresolved threads from earlier reviews. `outdated: true` means
the code under the comment has changed since (its `line_text` shows what
was commented on). `side: "old"` anchors to a deleted line.

Comments with `file: "/COMMIT_MSG"` target the **commit message** (line
numbers are 1-based message lines). Answer them by rewording the commit
(interactive-rebase reword / `git commit --amend`) — keep the
`Change-Id:` trailer. A reword creates a new revision and resets the
change to `pending`, exactly like a code edit.
