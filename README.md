# nit

Commit-level code review for AI coding agents.

An author registers a chain; you review each commit gerrit-style — draft
line comments, approve or request changes; the author resumes on your
feedback, amends the reviewed commit in place and pushes again — the
`Change-Id:` trailer keeps its identity, the rewrite becomes a new
revision. Merged or abandoned chains drop off the dashboard on their own.

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
nit push             # register the current chain for review
nit wait             # block until the reviewer acts; prints feedback JSON
# fix → amend the commit (keep its Change-Id) → nit push → nit wait → …
# all approved → merge; chain disappears
```

With a cooperative monitor, tail instead of blocking on `nit wait`:

```sh
nit log --follow --reviewer-only   # stream reviewer activity as it lands
```

Details for agents: the `nit` plugin's `lifecycle` and `comment` skills, and
`nit --help`.

## Hacking

Read [CLAUDE.md](CLAUDE.md) (humans welcome too), then the docs it points
at. Everything — dev, tests, builds — runs inside the flake devShell.
