## Push

- `POST /api/push` — register a tip for review (idempotent; this is
  `nit push`).

  ```json
  req:  {"git_dir": "/abs/path/.git", "tip": "f3a9…", "base": "main",
         "partial": true}
  resp: PushResult (below)
  ```

  `git_dir` is the repo's canonical **git-common-dir** (`git rev-parse
--git-common-dir`), canonicalized server-side; the `nit` CLI infers it from
  the cwd. `base` configures the repo's canonical branch: recorded on the
  repo's first push, it must equal the stored `base_branch` on every push
  after — a different base is a **400** (one canonical branch per repo).
  `base` is **optional**: omitted, a registered repo reuses its stored
  `base_branch`, and a fresh repo auto-detects the local `main` or `master`
  — a **400** asking the caller to specify `base` when neither or both exist.
  `tip` is any
  ref or rev, resolved to a commit at push time (the CLI sends the resolved
  commit sha of its checked-out HEAD by default); git is the source of truth
  for branch position, nit stores no branch sha.

  The server walks `merge-base(base, tip)..tip` oldest-first and, for each
  commit, **upserts the change** (keyed by its `Change-Id`) and **appends a
  `revision` entry iff the commit-sha moved** (a pure rebase — patch-id-equal
  with an unchanged message — appends a revision but does not reset review
  status). The walk is **all-or-nothing**: a `400` rejects the whole push on
  any structural fault (a merge or root commit, a commit missing its
  `Change-Id` trailer, a duplicate trailer within the walk, a `fixup!`/
  `squash!` subject, or a commit-sha already recorded under a different
  change). A push that would add a revision to an **abandoned** change is a
  **409** — reopen it first (`nit reopen`).

  `partial` is optional and sticky: `true` marks the tip's latest revision
  partial (`nit push --partial`), `false` clears it (`nit ready`), absent
  leaves it unchanged. A push that walks to nothing (`tip` is ancestor-or-equal
  of `base`) is a **409** — the tip is already merged into the base (or is the
  base itself), so there is nothing to review. A re-push where the walk is
  non-empty but nothing moved is **idempotent** (200), so a crash-retry is safe.

```json
PushResult = {
  "tip_change": {"change_id": 10, "change_key": "I3f2…",
                 "revision": 2, "status": "pending"},
  "chain": Chain    // tip-rooted: the derived path, each member at the
                    // revision this push gave it (see "Chains")
}
```

There is no chain id — a chain is addressed by its **tip change id** plus an
optional `?revision` selecting the patchset (and hence the chain context).
