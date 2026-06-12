# Agent workflow — how coding agents use nit

nit reviews **commits**, not branches. Make each commit one reviewable unit
(one logical change, own subject + body). The branch you register is the
"chain"; every commit on it becomes a change the human reviews separately.

## Conventions for your commits

- Add a `Change-Id: I<unique-token>` trailer (any opaque unique string;
  40 hex like gerrit is customary) to every commit message — and never the
  same token twice. That keeps a change's identity — and its comment
  history — stable across rebases and amends. Without it nit falls back to
  patch-id/subject matching, which breaks if you rewrite both the diff and
  the subject at once. (Duplicated trailers are flagged in the push
  response `scan_warnings`; fix them.)
- **Never merge into your branch** — no `git pull` without `--rebase`. A
  merge commit in the chain fails the scan (`last_scan_error`); rebase onto
  the base instead.
- Answer review feedback with **`fixup!` commits**:
  `git commit --fixup=<sha of the reviewed commit>`. nit folds the fixup
  into that change as a new revision; the reviewer sees what you changed
  (interdiff), not a pile of "address review comments" commits. Prefer
  `fixup!` over `squash!` (squash! needs interactive message editing and
  draws a warning).
- After approval, autosquash before merging (git ≥ 2.44):
  `GIT_EDITOR=true git rebase --autosquash <base>`
  (fallback: `GIT_SEQUENCE_EDITOR=: GIT_EDITOR=true git rebase -i --autosquash <base>`).

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
# read feedback; for each comment: fix → git commit --fixup=…,
#   or answer with: nit reply <comment-id> [--resolve] -m "…"
nit push                      # fixups become new revisions
#   ⚠ check the push output: any change with needs_rebase:true means a
#   fixup conflicted — restructure (rebase/autosquash the chain) and push
#   again before waiting.
nit wait                      # …repeat until state=ready_to_merge
# then: GIT_EDITOR=true git rebase --autosquash <base>; merge/ff the branch
nit push                      # optional: next scan marks the chain merged
```

- `nit push [--partial] [--base <ref>] [--branch <name>] [--server <url>]`
  — defaults: branch = current HEAD branch, base = `main` (falls back to
  `master`), server = `$NIT_SERVER` or `http://127.0.0.1:8877`. Prints the
  chain JSON including `web_url` — tell the human where to review. Exit
  ≠ 0 on scan errors; re-running is always safe (idempotent). `--partial`
  marks the chain partial: review can start, merging cannot. Sticky — a plain
  push never clears it. Feedback can land mid-build: each push response
  carries the change statuses, and `nit status` shows the full Feedback
  JSON without blocking — handle it as normal `agents_turn` work
  (fixups/replies below), folded into the next incremental push.
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
  `comments` (fix every comment with a fixup, or reply/`--resolve` with
  reasoning); any change with `needs_rebase: true`: restructure the chain.
  Then `nit push` and wait again. `commented` means the reviewer asked
  questions without blocking — reply, don't just wait.
  Exception: on a partial chain (`chain.partial: true`) with **no**
  `changes_requested`/`commented`/`needs_rebase` entries, `agents_turn`
  just means every pushed change is approved — the reviewer is caught up.
  Not an error, nothing to address: keep pushing commits, or `nit ready`
  when the branch is done.
- `ready_to_merge` — every change approved: autosquash-rebase onto base,
  merge/ff, done. The chain leaves the dashboard on the next scan.
- `waiting_for_review` — nothing actionable (the poll timed out); wait
  again.
- `merged` / `abandoned` — the chain is closed; stop.

Comments in feedback are scoped to each change's **latest review**, plus
any still-unresolved threads from earlier reviews. `outdated: true` means
the code under the comment has changed since (its `line_text` shows what
was commented on). `side: "old"` anchors to a deleted line.

Comments with `file: "/COMMIT_MSG"` target the **commit message** (line
numbers are 1-based message lines). Answer them by rewording the commit
(interactive-rebase reword / `git commit --amend`), not with a `fixup!` —
a fixup can't change the message. A reword creates a new revision and
resets the change to `pending`, exactly like a code edit.
