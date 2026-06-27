# nit

Commit-level code review for AI coding agents.

Agents register a branch; you review each commit gerrit-style — draft line
comments, approve or request changes; agents resume on your feedback,
amend the reviewed commit in place and push again — the `Change-Id:`
trailer keeps its identity, the rewrite becomes a new revision. Merged or
abandoned branches drop off the dashboard on their own.

## Run

```sh
nix build            # → result/bin/nit (server + CLI, web UI embedded path)
nix develop          # devShell with the full toolchain
```

```sh
nit serve            # review UI + API on http://127.0.0.1:8877
nit --version        # client + server build; non-zero exit if the server is down
```

Agent loop (any tool that can run shell commands):

```sh
nit push             # register current branch for review
nit wait             # block until the reviewer acts; prints feedback JSON
# fix → amend the commit (keep its Change-Id) → nit push → nit wait → …
# all approved → merge; chain disappears
```

With a cooperative monitor, tail instead of blocking on `nit wait`:

```sh
nit log --follow --reviewer-only   # stream reviewer activity as it lands
```

Details for agents: [docs/agent-workflow.md](docs/agent-workflow.md).

## Hacking

Read [CLAUDE.md](CLAUDE.md) (humans welcome too), then the docs it points
at. Everything — dev, tests, builds — runs inside the flake devShell.
