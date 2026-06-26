## Repos

A repo is the registry grouping for changes; its identity is the
**git-common-dir** (the `.git` dir, shared across worktrees), which is also
its display name. A repo has exactly **one canonical base ref** (`base_ref`)
— any git ref (a local branch, `origin/main`, a tag, a sha) that resolves to
a commit; mergedness is always tracked against it, there is no
land-anywhere. The web main page lists repos, each linking to that repo's
chains. A repo is
registered explicitly with `nit repo create` (`POST /api/repos`); a
`nit push` into an unregistered repo is rejected (404).

- `POST /api/repos` — register a repo, configuring its canonical base ref
  (`nit repo create`).
  ```json
  req:  {"git_dir": "/abs/path/.git", "base": "origin/main"}
  resp: Repo
  ```
  `git_dir` is canonicalized and must open as a git repo. `base` is
  required and must resolve to a commit — any git ref, e.g. `origin/main`
  — a **400** otherwise; nit never guesses the base. **409** if the git
  dir is already registered.
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
  "base_ref": "main",            // the one canonical base ref; mergedness tracks it
  "active_chains": 2             // live tip count (derived from the tip set)
}
```
