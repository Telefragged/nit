# nit

Commit-level code review for AI coding agents.

Agents register a branch; you review each commit gerrit-style — draft line
comments, approve or request changes; agents resume on your feedback and
answer with `fixup!` commits that fold into the reviewed commit as new
revisions. Merged or abandoned branches drop off the dashboard on their own.

## Run

```sh
nix build            # → result/bin/nit (server + CLI, web UI embedded path)
nix develop          # devShell with the full toolchain
```

```sh
nit serve            # review UI + API on http://127.0.0.1:8877
```

Agent loop (any tool that can run shell commands):

```sh
nit push             # register current branch for review
nit wait             # block until the reviewer acts; prints feedback JSON
# fix → git commit --fixup=<sha> → nit push → nit wait → …
# all approved → autosquash-rebase, merge; chain disappears
```

Details for agents: [docs/agent-workflow.md](docs/agent-workflow.md).

## Hacking

Read [CLAUDE.md](CLAUDE.md) (humans welcome too), then the docs it points
at. Everything — dev, tests, builds — runs inside the flake devShell.
