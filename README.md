# nit

Commit-level code review for AI coding agents.

An author pushes a stack of commits; you review each one gerrit-style — draft
line comments, approve or request changes; the author resumes on your
feedback, amends the reviewed commit in place and pushes again — the
`Change-Id:` trailer keeps its identity, the rewrite becomes a new
revision. Merged or abandoned changes drop off the dashboard on their own.

## Run

```sh
nix build            # → result/bin/nit (server + CLI, web UI embedded path)
nix develop          # devShell with the full toolchain
```

```sh
nit serve            # review UI + API on http://127.0.0.1:8877
nit --version        # client + server build; non-zero exit if the server is down
```

Author loop (any tool that can run shell commands):

```sh
nit push             # register the checked-out commit and its chain for review
nit log --wait 0     # block until the reviewer acts; prints the digest and the entries
# fix → amend the commit (keep its Change-Id) → nit push → nit log --wait N → …
# all approved → merge
```

A push tags the changes it registers with the harness session, the worktree
and the branch it ran from. `nit status` and `nit log` read by those tags:
the session by default, else the worktree, else the branch. So an agent
never names a tag. `--tag` names any tag instead. `--tag` on a push adds
your own, and any key you don't name keeps the value the change already
carries:

```sh
nit push --tag feature=epic-saga
nit status --tag feature=epic-saga
```

```
tag feature=epic-saga
12  Ia1b2c3d4  changes_requested  r2  3u  server: add health endpoint
13  Ie5f6a7b8  pending            r0  0u  web: render the diff
```

`GET /api/changes?repo={id}&tag=feature=epic-saga` selects on them
(repeatable, exact key and value, every one must match),
`GET /api/log?repo={id}&tag=…` reads their log as one, and
`GET /api/tags?repo={id}` lists the keys and values in use.

With a cooperative monitor, tail instead of blocking on `--wait`. The
monitor follows the session, new commits included:

```sh
nit log --follow --incoming   # stream the reviews and the lifecycle as they land
```

Details for agents: the `nit` plugin's `lifecycle` and `comment` skills, and
`nit --help`.

## Hacking

Read [CLAUDE.md](CLAUDE.md) (humans welcome too), then the docs it points
at. Everything — dev, tests, builds — runs inside the flake devShell.
