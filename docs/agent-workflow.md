# Agent workflow — how coding agents use nit

nit reviews **commits**, not branches. Make each commit one reviewable unit
(one logical change, own subject + body). The branch you register is the
"chain"; every commit on it becomes a change the human reviews separately.

## Conventions for your commits

- Add a `Change-Id: I<unique-token>` trailer (any opaque unique string;
  40 hex like gerrit is customary) to every commit message. That keeps a
  change's identity — and its comment history — stable across rebases and
  amends. Without it nit falls back to patch-id/subject matching, which
  breaks if you rewrite both the diff and the subject at once.
- Answer review feedback with **`fixup!` commits**: `git commit --fixup=<sha
  of the reviewed commit>`. nit folds the fixup into that change as a new
  revision; the reviewer sees what you changed (interdiff), not a pile of
  "address review comments" commits.
- After approval, autosquash before merging:
  `git rebase --autosquash --interactive=false <base>` (or
  `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash <base>`).

## The loop

```sh
nit push                      # register/refresh current branch (in repo cwd)
nit wait                      # block until the reviewer acts; prints JSON
# read feedback, fix code, git commit --fixup=…, then:
nit push                      # fixups become new revisions
nit wait                      # …until every change is approved
# all approved → rebase --autosquash onto base, merge/ff the branch
nit push                      # optional: next scan marks the chain merged
```

- `nit push [--base <ref>] [--branch <name>] [--server <url>]` — defaults:
  branch = current HEAD branch, base = `main` (falls back to `master`),
  server = `$NIT_SERVER` or `http://127.0.0.1:8877`. Prints the chain JSON
  including `web_url` — tell the human where to review.
- `nit wait [--timeout <secs>]` — returns immediately when feedback is
  already actionable, else long-polls. Exit 0 with JSON on stdout.
- `nit status` — current chain state without blocking.

## `nit wait` / feedback JSON

```json
{
  "state": "agents_turn",   // agents_turn | ready_to_merge | waiting_for_review
                            // | merged | abandoned
  "chain": {"id": 1, "branch": "feat/x", "base": "main", "web_url": "…"},
  "changes": [
    {
      "change_id": 10, "change_key": "I3f2…", "subject": "…",
      "commit_sha": "…", "revision": 2,
      "status": "changes_requested",
      "review": {"verdict": "request_changes", "message": "cover msg"},
      "comments": [
        {"file": "src/api/mod.rs", "line": 41, "side": "new",
         "body": "this unwrap can panic on …", "revision": 2}
      ]
    }
  ]
}
```

Interpretation:
- `agents_turn` — at least one change has `request_changes` on its latest
  revision. Fix every comment (or argue in the commit message of the fixup),
  push, wait again.
- `ready_to_merge` — every change approved: autosquash-rebase onto base,
  merge, done. The chain disappears from the dashboard on the next scan.
- `waiting_for_review` — nothing actionable (wait timed out); wait again.
- Comments are anchored to the revision they were written on; `side: "old"`
  means the deleted line.
