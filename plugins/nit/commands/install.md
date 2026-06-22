---
name: install
description: Set nit up for this project — make the nit CLI reachable (install to PATH or run on demand), verify the server, ensure commits get a Change-Id, then record the project's approve action in CLAUDE.md/AGENTS.md so agents drive every change through review. Use on "set up nit", "install nit", or first-time onboarding of a repo to nit.
---

# /nit:install — onboard this project to nit

Setup the user initiates. Walk through the decisions below, then write the
result into the project's agent config. This is the interactive command — it
asks the user directly, unlike `/nit:goal`, `/nit:fork`, and `/nit:plan`, which
never ask and route everything through `nit comment`.

## 1. Make the `nit` CLI reachable

Ask the user (AskUserQuestion) how they want `nit` available:

- **Install to PATH** — run `nix profile add github:Telefragged/nit`. After
  this, `nit` is a normal command anywhere. Best when nit is used often.
- **Run on demand** — no install; invoke `nix run github:Telefragged/nit -- <args>`
  each time (e.g. `nix run github:Telefragged/nit -- push --partial`). Best
  for a one-off or to avoid touching the user's profile.

Carry the choice through the rest of setup: everywhere below that says `nit`,
the on-demand path is `nix run github:Telefragged/nit --`.

Verify it resolves:

```sh
nit --version                              # PATH install
nix run github:Telefragged/nit -- --version   # on demand
```

## 2. Confirm the server is up

A push needs a running server. Check it (don't start one — the server and its
database belong to the user):

```sh
curl -fsS "${NIT_SERVER:-http://127.0.0.1:8877}/api/health"
```

If it's down, tell the user to start it (`nit serve`, or
`nix run github:Telefragged/nit -- serve`) and point `$NIT_SERVER` at it if
it's not on the default `http://127.0.0.1:8877`.

## 3. Register the repo

A repo must be registered before anything can be pushed — a `nit push` into an
unregistered repo is rejected (404). Register it once, pinning its canonical
base branch (the branch mergedness is tracked against):

```sh
nit repo create --base <branch>
```

`--base` is **required** and must name an existing branch — nit never guesses
it, and **neither do you**. You **MUST ask the user (AskUserQuestion) to choose
the base branch** before registering — never assume or infer it, not even when
the repo has a single branch. Detect the likely candidates (the current
branch, `main`/`master`, `git symbolic-ref refs/remotes/origin/HEAD`) and offer
them as the suggested options, but the choice is the user's. A 409 means the
repo is already registered — nothing to do.

## 4. Make sure commits get a Change-Id

nit identifies a change by a `Change-Id` trailer on its commit — the stable
id that survives amends and rebases — so every pushed commit needs one. It
should be added **automatically**, not left for an agent to remember.

**Probe first** — does the repo already add one? A gerrit-style setup installs
a commit-msg hook here:

```sh
hook="$(git rev-parse --git-path hooks/commit-msg)"
test -x "$hook" && grep -q Change-Id "$hook" && echo "already handled"
```

If commits already come out with a `Change-Id:` trailer, you're done.

If nothing adds one, ask the user (AskUserQuestion) which they'd prefer:

- **Install a commit-msg hook (recommended)** — appends a trailer to any
  commit that lacks one, so it is never forgotten. `git interpret-trailers`
  places it in the commit's trailer block correctly:

  ```sh
  hook="$(git rev-parse --git-path hooks/commit-msg)"
  cat > "$hook" <<'EOF'
  #!/bin/sh
  # nit: add a Change-Id trailer when one is absent.
  grep -q '^Change-Id:' "$1" && exit 0
  id="I$(od -An -N20 -tx1 /dev/urandom | tr -d ' \n')"
  git interpret-trailers --in-place --trailer "Change-Id: $id" "$1"
  EOF
  chmod +x "$hook"
  ```

- **Document it in the agent instructions** — if a hook isn't wanted, append
  to CLAUDE.md/AGENTS.md that every commit needs a `Change-Id: I…` trailer in
  its final trailer block (no blank line splitting that block), generated
  with:

  ```sh
  python3 -c 'import secrets; print("I"+secrets.token_hex(20))'
  ```

## 5. Record the approve action

Ask the user (AskUserQuestion) for this project's **approve action**: the
steps to take once a change reaches the `approved` state — how this project
lands approved work (e.g. rebase-and-fast-forward, merge the PR, run a deploy
script). This is project-specific; nit does not prescribe it.

Then write a short section into the project's agent config so every agent
knows to route work through review. Pick the file:

- If `CLAUDE.md` exists, append to it.
- Else if `AGENTS.md` exists, append to it.
- Else ask the user which to create.

Append (filling in `<base>` with the registered base branch and their approve
action verbatim):

```markdown
## Reviewing changes with nit

This project uses [nit](https://github.com/Telefragged/nit) for code review.
Drive every change through review rather than landing it directly: push each
completed commit with `nit push --partial`, answer reviewer feedback by
amending in place and pushing again, and keep questions and decisions in nit
as comments. Say "drive it through nit" to take a change through the loop; the
`/nit:goal`, `/nit:fork`, and `/nit:plan` commands are opinionated shortcuts.

Never build a change you'll push for review on `<base>` itself — that is the
canonical branch nit watches for landings, so pushed commits must not live on
it.

**Approve action** — when a change reaches the `approved` state, land it by:

<the approve action the user described>
```

## 6. Confirm

Report back: how `nit` is reachable, whether the server answered, that the
repo is registered (and its base branch), how Change-Ids are provided, and
which file you wrote the approve action into. Suggest the user try `/nit:goal`
on their next change.
