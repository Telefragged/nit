# nit

Commit-level code review for AI coding agents.

An author pushes a stack of commits; you review each one gerrit-style — draft
line comments, approve or request changes; the author resumes on your
feedback, amends the reviewed commit in place and pushes again — the
`Change-Id:` trailer keeps its identity, the rewrite becomes a new
revision. Merged or abandoned changes drop off the dashboard on their own.

## Install

On x86_64 Linux, install the latest release into `~/.local/bin`:

```sh
curl -fsSL https://github.com/Telefragged/nit/releases/latest/download/install.sh | sh
```

The script adds `~/.local/bin` to `PATH` in your shell startup files, and it installs the Claude Code plugin when Claude Code is installed. `nit update` installs the latest release again. A running `nit serve` restarts on the new build, and each `nit watch` follows it.

With nix, use the home-manager module:

```nix
# flake.nix
inputs.nit.url = "github:Telefragged/nit";

# home-manager configuration
imports = [ inputs.nit.homeManagerModules.default ];
programs.nit.enable = true; # nit on PATH, and the Claude Code plugin
services.nit.enable = true; # nit serve as a systemd user service
```

To update, run `nix flake update nit` and switch. The switch restarts the server, and each `nit watch` follows it.

## Run

```sh
nix build            # → result/bin/nit (server + CLI, web UI embedded path)
nix develop          # devShell with the full toolchain
```

```sh
nit serve            # review UI + API on http://127.0.0.1:8877
nit --version        # client + server build; non-zero exit if the server is down
nit gc               # delete old closed changes and unused attachments
```

The server deletes old data by age only on a `POST /api/gc` request, and
`nit gc` sends that request. The server then deletes each merged or
abandoned change whose last entry is older than `--retention-days` (30 by
default), and each attachment that no change links to. `nit gc` prints how
many changes and attachments the server deleted, and the total size of
those attachments. Run it from a scheduler, for example a systemd timer or
cron.

On `nit repo remove <path>`, the server deletes a repo and all of its
changes at once. It deletes their attachments on the next `nit gc`.

`nit repo set-ref <path> <ref>` points a repo at a new canonical ref, for
example from `main` to `origin/main`. The new ref must resolve to the
commit that nit last recorded for the current canonical ref.

Author loop (any tool that can run shell commands):

```sh
nit watch            # background (the Claude Code plugin starts it): post reviews to this session
nit push             # register the checked-out commit and its chain for review
# fix → amend the commit (keep its Change-Id) → nit push → …
# all approved → merge
```

A push tags the changes it registers with the harness session, the worktree
and the branch it ran from. `nit status` reads by those tags:
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
(repeatable, exact key and value, every one must match), and
`GET /api/tags?repo={id}` lists the keys and values in use.

`nit attach <file> --change <number>` puts a file that is not in the commit
on a change: a plan, a screenshot, a command's output, a rendered page. The
review page lists attachments below the commit's files, and renders Markdown,
images and HTML. A toggle on a Markdown or HTML file, attached or in the
commit, switches between its lines and the rendered whole file. An attachment
carries forward to later revisions until a new version under the same name
replaces it, and attaching unchanged bytes adds nothing. So a hook may attach a build's artifacts after every push. For
example, a Claude Code `PostToolUse` hook on `Bash`:

```sh
#!/bin/sh
# Attaches the web screenshots to HEAD's change after each `nit push`.
jq -e '.tool_input.command | test("\\bnit push\\b")' >/dev/null || exit 0
id=$(git log -1 --format='%(trailers:key=Change-Id,valueonly)')
for file in screenshots/*.png; do
  nit attach "$file" --change-id "$id"
done
```

`nit watch` follows the session, new commits included, and posts each review
and lifecycle change to the agent that started it. It needs a harness that
exports an inbox socket, or `--inbox <path>`. It exits when its parent process
exits. In Claude Code, the `nit` plugin starts it from a `SessionStart` hook.

Details for agents: the `nit` plugin's `lifecycle` and `comment` skills, and
`nit --help`.

## Hacking

Read [CLAUDE.md](CLAUDE.md) (humans welcome too), then the docs it points
at. Everything — dev, tests, builds — runs inside the flake devShell.
