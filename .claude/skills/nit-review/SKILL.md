---
name: nit-review
description: Land changes through nit's review loop (push --partial per commit → ready → wait → amend → approve action). Use as soon as the first commit on a feature branch is done — review runs alongside the build — on "push for review", or when acting on nit feedback. Exemptions: docs/dev.md.
---

# nit-review — the dogfood loop

The project this skill is installed in reviews its own changes with nit.
Protocol reference: `docs/agent-workflow.md`. This skill is the
operational checklist for driving it from a Claude session.

## Cadence — the rule to get right before anything else

**`nit push --partial` is part of finishing each commit**, with the same
standing as treefmt and the Change-Id trailer — never a phase that comes
after the branch. The moment a commit is green and committed, push it.
Nothing in the plan moves a push later:

- a planned cleanup / self-review / simplification / verification pass
  does not hold pushes back — push now, amend later; post-push amends
  become new revisions **by design**, and interdiffs show the reviewer
  exactly what the pass changed;
- a user instruction to add a later pass reorders that _pass_, not the
  pushes;
- coordinators never batch, centralize, or phase-gate workers' pushes.

If commit N+1 is being started while commit N is unpushed, the cadence
is already broken. An unpushed commit is invisible to the reviewer.

## Keep shipping — bite-sized, not batched

Review is asynchronous: the reviewer pulls your pushed commits and reads
them at their own pace, so a long chain of small commits is the goal, never
something to ration. Once a scope is agreed, drive it to completion — keep
producing and pushing commits — without halting at a milestone to ask
"shall I continue?". The reviewer is never blocked by _more_ commits; an
arbitrary checkpoint just stalls the work, the terminal-channel equivalent
of leaving a commit unpushed. Pause only for (a) a genuine decision that
forks what you would build (ask it, or raise it as an unresolved `nit
reply`), or (b) an explicit redirection from the user. "Turn length", "the
user might want to steer", and "a clean stopping point" are not reasons to
stop.

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
  branch/worktree and run the approve action (recipe: docs/dev.md "The
  approve action") exactly where the loop would have.
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
- Every commit: builds green first, treefmt-clean —
  `nix develop -c treefmt` before each commit, and again after any
  rebase (amend the churn in; docs/dev.md "Formatting") — one concern,
  and its own
  `Change-Id: I<40hex>` trailer — required; a missing or duplicated
  trailer (or a pushed `fixup!`/`squash!` commit) fails the scan. **All
  trailers in one block** — a blank line between `Change-Id:` and
  `Co-Authored-By:` splits the block and the trailer is silently lost
  (git last-paragraph rule). Generate:
  `python3 -c 'import secrets; print("I"+secrets.token_hex(20))'`.

## The loop

```sh
cursor=0            # 0-based: the count of log entries you've consumed
repo=$(pwd); branch=$(git branch --show-current)   # push/ready need both
# after EVERY completed commit (green, treefmt-clean, one concern, Change-Id'd):
nit push --partial --repo "$repo" --branch "$branch"  # register/refresh, partial
# → FIRST push starts review on commit one, not when the branch is done
# after the LAST commit:
nit ready --repo "$repo" --branch "$branch"   # clears partial; reach approved
nit wait $cursor    # blocks until entries land beyond $cursor; prints JSON
```

`--repo`/`--branch` are required — push has no cwd fallback (a stray push
from the wrong checkout would fork a duplicate chain). `nit log` selects
its chain differently: by `--chain <id>`, or absent it the cwd's
repo+branch — it does **not** accept `--repo`/`--branch`. Pass `--chain
<id>` for a monitor, whose long-lived command can't rely on an ambient
cwd. **Prefer the follow-monitor**: run
`nit log --follow --oneline --reviewer-only --chain <id> $cursor` under
the **Monitor tool** (`persistent: true`) instead of polling
`nit wait` — Monitor turns each relayed line into a notification, so you
triage entries as they land (act on follow-ups now, queue unrelated
comments). **Use the Monitor tool, _not_ `Bash` with `run_in_background`
for the follow-monitor**: a background Bash command only notifies you when
it _exits_, and `nit log --follow` never exits on its own, so a
background-Bash follow streams into a file that nothing ever wakes you to
read — the exact trap that looks like a working watcher but is a dropped
review. (`nit wait`, which _does_ exit on each wake, is the one that belongs
in a background Bash task — see below.) Keep `--oneline`: one parseable line
per entry, not the token-heavy multi-line full JSON. `--reviewer-only` mutes your own echoes (`revisions`, `comment`,
`partial`) and otherwise applies `nit wait`'s wake rule — it wakes on the
reviewer and chain closure, but not on a comment-less approve that leaves
the chain short of `approved`. Each relayed line is still just a doorbell:
re-read the gap with
`nit log $cursor..` from your last-consumed index, act on all of it, then
advance `$cursor`, never on the one printed entry alone. Track the index
you **consumed from `nit log`**, not the one the monitor printed; resume
after a restart with `nit log --follow --reviewer-only --chain <id> $cursor`. `nit wait` is the fallback when
a monitor is not available (docs/agent-workflow.md "Following the log
instead of waiting").

**A running watcher is mandatory — never finish a turn with the chain open
and nothing watching it.** The loop does not end at `nit ready` or at a
`nit push`; it ends when the chain closes (`merged`/`abandoned`). The
moment you `nit ready` (or push the last revision), a watcher must be
running and stay up until the chain closes — either a `nit log --follow`
under the **Monitor tool** (`persistent: true`; the preferred form above)
or, as the fallback, `nit wait $cursor` as a **background Bash task** (it
exits on each wake, which re-invokes you). A `ready`/pushed chain with no
watcher is a dropped review, as broken as an unpushed commit. Re-arm
`nit wait` after every push and comment; a Monitor follow stays up on its own
(stop it with TaskStop once the chain closes).

`nit wait $cursor` returns `{head, entries, state, …}`. **Advance the
cursor only from that result** (`cursor=<head>`); `push`/`comment` return no
index at all (just a `Chain`/`Thread`, no `head`), so the cursor can only
ever come from `wait`/`log`. That is what guarantees a reviewer comment
landing between two of your own pushes is never skipped. After every push/comment, the next
`nit wait $cursor` returns immediately (your own just-appended entries);
process, advance, wait again until it actually blocks. Skim with
`nit wait --oneline $cursor`; inspect specific entries without moving the
cursor via `nit log <ranges>`. Branch on `state`:

- **`agents_turn`** — for each change with `request_changes`/`commented`:
  - code feedback → fix it by amending the commit in place, keeping its
    Change-Id: `git commit --fixup=<commit_sha of the change>`, then
    `GIT_EDITOR=true git rebase --autosquash "$(git merge-base main HEAD)"`
    — squash **before** pushing (pushed `fixup!` commits fail the scan).
    Run treefmt before committing the fixup so the fix lands formatted;
    after the autosquash — and any other rebase, doubly so one with
    conflicts — re-format every rewritten commit with the docs/dev.md
    "Formatting" rebase recipe (amending the tip alone misses churn in
    earlier commits).
    Then `nit comment --change-id <Change-Id> --thread <thread-id> --resolve -m "what you did"`;
  - questions → `nit comment --change-id <Change-Id> --thread <thread-id>` with
    the answer (`--resolve` when settled);
  - Then `nit push` (the rewritten commits become new revisions) and wait
    again.
  - On a partial chain, `agents_turn` with none of the above (every pushed
    change approved) is not an error and not feedback — the reviewer is
    caught up. Keep building, or `nit ready` when the branch is done.
- **`approved`** — every change approved. nit's job ends here; what to do
  with an approved chain is **the project's approve action**, not part of
  the loop — run it as the project defines it (recipe: docs/dev.md "The
  approve action", covering the rebase, ordering, and the worktree
  caveat). If the landing isn't yours to drive (main lives in another
  checkout a coordinator owns), stop at `approved` and report to the
  coordinator.
- **`merged` / `abandoned`** — chain is closed; stop.
- **`waiting_for_review`** — nothing actionable: `nit wait` woke on your
  own just-pushed entries. Advance the cursor and wait again.

Never submit a review verdict yourself (`POST /api/changes/*/reviews` is
the human's side). The agent surface is push / ready / wait / log /
status / comment.

Read rolled-up state through these verbs — `nit status` (`--oneline` to
skim where the chain stands) and the `wait`/`log` `--oneline` digests —
not by curling the HTTP API and hand-parsing JSON; reach for the raw API
only when the CLI genuinely lacks the data you need.

## Notes

- The cursor is yours to track (start `0`, advance to each `wait`/`log`
  `head`). Re-waiting right after a push returns immediately with your own
  `revisions` entry — that is expected; keep advancing until `wait`
  blocks. A comment-only verdict you answer with comments alone does **not**
  re-spin: your comment is just another entry, and the next `wait` blocks.
- **Annotate the choices you make instead of asking the human.** When you
  make a non-obvious call mid-build — a new dependency, one approach over
  alternatives — open a thread on the exact lines with
  `nit comment --change-id <Change-Id> --file … --line … [--range …] -m "…"`:
  `--resolve` for a settled note that needs no response, left **open**
  (default) for a choice the reviewer should weigh in on. Make the call,
  annotate it, keep going (docs/agent-workflow.md "Annotate the choices you
  make").
- **The review conversation lives in nit, not this session.** When you do
  have a question or a design choice for the reviewer, raise it with
  `nit comment --change-id <Change-Id> --thread <thread-id> -m "…"` on the thread it concerns (or a
  new one via `--change-id`), leave it unresolved, re-arm `nit wait`, and carry
  on — never block the human session on the answer. Your terminal is the
  channel only when the user prompts you there directly
  (docs/agent-workflow.md "Where the conversation happens").
- If a push fails with a Change-Id scan error (missing or duplicate
  trailer, or a `fixup!`/`squash!` commit), fix the commit messages and
  push again — a blank line splitting the trailer block is the usual
  culprit for "missing".
