## Repos

A repo is the registry grouping for changes; its identity is the
**git-common-dir** (the `.git` dir, shared across worktrees), which is also
its display name. A repo has exactly **one canonical branch** (`base_branch`)
— mergedness is always tracked against it, there is no land-anywhere. The web
main page lists repos, each linking to that repo's chains. Repos are created
lazily by the first `nit push`; there is no separate registration step.

- `GET /api/repos` → `{"repos": [Repo]}` — registration order.
- `GET /api/repos/{id}` → Repo — one repo by id; 404 if unknown.
- `PATCH /api/repos/{id}` — repoint a repo at a new git-common-dir after it
  moved on disk (`nit repo move`).
  ```json
  req:  {"git_dir": "/new/path/.git"}
  resp: Repo
  ```
  `git_dir` is canonicalized and must open as a git repo. 404 if the repo is
  unknown, 400 if the new path can't be resolved, 409 if it already belongs
  to another repo.

```json
Repo = {
  "id": 1,
  "git_dir": "/abs/path/.git",   // canonical git-common-dir — identity + name
  "base_branch": "main",         // the one canonical branch; mergedness tracks it
  "active_chains": 2             // live tip count (derived from the tip set)
}
```
