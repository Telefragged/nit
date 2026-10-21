# Development

## Verification

Checks verify a change, not a green build — `nix build` skips tests
(crate2nix builds the crate with `runTests` off). Before every commit
(golden rule 9):

- `nix develop -c cargo check` — fast inner-loop gate.
- `nix flake check` — the pre-commit gate: builds the product and runs the
  Rust validators (`test`, `test-nit-types`, `test-nit-wasm`) and the web
  validators (`web-lint`, `web-test`, `web-screenshots`). Every crate of ours
  compiles with `clippy-driver` under `-D warnings`, so a lint fails any
  build; the `test*` checks add the test targets and run rustdoc over the
  crate, so a doc test failure or a broken intra-doc link fails there too;
  `web-screenshots` runs the screenshot harness against the nix browsers, so
  a driver/npm-pin skew fails here. Run
  one alone with `nix build .#checks.<system>.test` or
  `.#checks.<system>.web-test`.
- `scripts/check-commits.sh <base>`: runs `nix flake check` on every
  commit in `<base>..HEAD`, 8 at a time. CI runs the same checks on every
  commit of a PR.

The crate2nix build file `Cargo.nix` is checked in. After any `Cargo.lock`
change, regenerate it with `nix develop -c crate2nix generate` and commit it
in the same change — a stale `Cargo.nix` fails the build fast. Likewise,
changing `web/package-lock.json` means refreshing `npmDepsHash` in
`flake.nix` (`nix run nixpkgs#prefetch-npm-deps -- web/package-lock.json`
prints it); a stale hash breaks `nix build` and every
`nix run '…?ref=main#nit'` CLI invocation.

The web's wire types (`web/src/api/types.gen.ts`) are generated the same
way — from `crates/nit-types` via `nix run .#gen-types`, a native
`cargo test` that runs the crate's `ts`-feature `ts-rs` exporter (no wasm).
Regenerate and commit it whenever those types change; the `types-drift`
check fails a stale file.

`nix build .#contract` builds the public contract. `openapi.json` is the
OpenAPI document that the routes' `#[utoipa::path]` annotations produce, and
the router registers its routes from the same annotations. `cli.json` is the
syntax of every `nit` command: its flags, positionals and subcommands.
`nix build -f release --argstr last <tag> '^*'` compares it with the
contract at the release tag `last`, and fails when the version in
`Cargo.toml` is too small a step for the changes. Before 1.0.0, a breaking
change needs the next minor version and any other change the next patch.
`result` holds the kind of the changes, and `result-changes` lists them. `nix run .#bump-oasdiff`
moves the pinned oasdiff to its latest release.

The shared change fold is compiled to WebAssembly (`crates/nit-wasm`) for the
event-driven change page. `gen-wasm` writes the glue + `.wasm` into
`web/src/wasm/` — **gitignored** (binary, derived) and injected into every web
nix build. It's on `PATH` inside the devShell (also runnable standalone as
`nix run .#gen-wasm`); every `npm run` frontend script generates it on demand,
so there's no manual step. The `wasm-build` check compiles it under `nix
flake check`. The `wasm-bindgen-cli` in the devShell and `crates/nit-wasm`'s
`wasm-bindgen` dep are pinned to the same version — a skew breaks the glue at
init, not at build.

## Formatting

`nix develop -c treefmt` formats the tree (`nix fmt` is the same; config in
`treefmt.toml`); the `treefmt` flake check verifies it, so `nix flake check`
fails on any unformatted file.

Format **per commit**, so each commit is clean on its own. A rebase breaks
this — replayed commits and conflict resolutions land unformatted in
whichever commit they touch, out of the tip's reach — so re-format every
rewritten commit after a rebase:

```sh
git rebase -x 'nix develop -c treefmt && if ! git diff --quiet; then git commit -a --amend --no-edit; fi' \
  "$(git merge-base main HEAD)"    # when merging: onto main instead
```

Two edges: don't run a bare treefmt before amending a checked-out historic
commit mid-rebase (it folds later commits' formatting into the amend —
stage only your files); and keep inline code spans on one line, or prettier
drops the hanging indent of a wrapped markdown list item.

## Screenshot harness (frontend checking for AI agents)

Agents read PNGs, not browsers. The harness renders every UI state from
fixtures with no backend, writing `screenshots/*.png` (repo root,
gitignored); run it and `Read` them:

```sh
cd web && nix develop -c npm run screenshots
```

It covers detailed states (drafts, 409s, interdiff); add a capture with
every new page or state. Each capture also writes `screenshots/<name>.html`:
the same state as one file, with its CSS inlined and its scripts removed.
Attach it to a UI change so that the reviewer reads the page in nit:

````sh
nit attach screenshots/review-attachments.html --change <number>
``` The npm `@playwright/test` version must match
`pkgs.playwright-driver` (the devShell sets `$PLAYWRIGHT_DRIVER_VERSION`) —
the `web-screenshots` flake check runs it in the sandbox and its
output _is_ the PNGs, so `nix build .#checks.<system>.web-screenshots` both
catches a skew and gives you the gallery in `result/`.

## Testing expectations

- Rust: unit tests beside the code; scan/identity logic gets real-git
  integration tests (`tempfile` + git2). `cargo test` runs as the `test`
  flake check.
- Frontend: tsc-clean always; test break-prone logic (diff rendering,
  comment anchoring) with vitest (`npm test`), which runs as the `web-test`
  flake check.
- A fresh `.worktrees/*` checkout has no `web/node_modules`; run
  `cd web && nix develop -c npm ci` before any web check.

## Commit & branch discipline

- **Hard-wrap the commit message at 72 columns.** This is not optional and is
  checked in review. The subject is a single line stating the _what_ (for
  indexing); after a blank line, the body explains _why_ as 72-column-wrapped
  prose — each line broken at ≤72 like a paragraph, **never one long line you
  let the terminal soft-wrap**. With `git commit`, write the body across real
  newlines (a `-m` per paragraph, lines pre-wrapped), not a single sentence.
- Keep messages **timeless** — no process narration ("rebased onto X", branch
  ordering); that goes in the `nit` reply or terminal, not git history.
- Never mix refactors with behavior changes.
- Keep the web dependency list short; justify any addition in the commit
  message.
- **Every change starts in its own worktree** on a `track/*` branch (golden
  rule 6), so `main` stays put and chains never serialize on a shared branch:

  ```sh
  git worktree add .worktrees/<slug> -b track/<slug> main
````

Address the worktree explicitly — absolute paths,
`cargo --manifest-path <worktree>/crates/nit/Cargo.toml`,
`git -C <worktree>` — never an ambient cwd, which may have drifted back
to the primary checkout. Commit there, drive the review loop, merge via
the approve action, then `git worktree remove .worktrees/<slug>` and
delete the branch ("Merging changes").

- **Parallel chains stay independent**: never pre-merge in-flight branches
  into a shared integration branch — each is built and reviewed on its own,
  conflicts resolved only as each merges. (Rebasing one in-flight branch
  onto a moved `main` is fine.)

## Merging changes — the nit review loop

This repo dogfoods nit: push finished work as a chain, a human reviews it in
nit, and the approve action merges it onto `main` through a GitHub PR. Drive
the loop with the `nit:lifecycle` skill, all the way to `merged`. Run the
`nit` CLI from the build that matches the running server (normally `main`'s:
`nit` on PATH, else `nix run '…?ref=main#nit'`), not your branch's binary.

`main` accepts no direct push. When every change in the chain is `approved`,
the approve action is:

1. Push the branch and open a PR:

   ```sh
   git push origin HEAD:track/<slug>
   gh pr create --base main --head track/<slug>
   ```

   A push that changes `.github/workflows/` needs a token with the `workflow`
   scope.

2. Enable auto-merge on the reviewed head. GitHub merges the PR when the
   required checks, `nix` and `semver`, pass:

   ```sh
   gh pr merge <pr> --auto --rebase --match-head-commit "$(git rev-parse HEAD)"
   ```

3. Wait for the merge with `scripts/wait-pr.sh <pr>`. It checks the PR every
   10 seconds. It exits 0 when the PR merged, and 1 when a check failed or
   the branch is behind `main` or conflicts with it. In Claude Code, run it
   in a background shell with a one-hour timeout. If the branch is behind
   `main` or conflicts with it, rebase the chain onto `main`, `nit push`,
   push the branch again, and repeat step 2.
4. Fast-forward the primary checkout's `main` to `origin/main`. nit marks
   the chain `merged` when it finds the merged commits there. The rebase
   merge gives each commit a new SHA, so delete the branch with
   `git branch -D track/<slug>` once `git diff track/<slug> main` is empty.

### Review exemptions

Golden rule 6 is the default: every change runs through nit, in a worktree,
regardless of size. The only exits are an explicit, up-front "skip nit" /
"merge directly" from the user for that change, or a standing entry below.

Standing exemptions (same discipline, still green):

- _(none yet)_

## Releases

A release starts from one edit: raise `workspace.package.version` in `Cargo.toml`. Then run `nix develop -c cargo check` to update `Cargo.lock` and `nix develop -c crate2nix generate` to update `Cargo.nix`, and open a PR. Run `nix run .#release-check` to see the result of the CI check before you push.

The `semver` job runs `nix run .#release-check` on every PR. When the version differs from the newest release tag, the script checks it against the contract changes since that tag (see "Verification"). Release tags are the bare version, for example `0.2.0`.

After the merge, `.github/workflows/release.yml` runs the same check. When the commit is a release, it builds `.#nit-release`, creates the tag and the GitHub release, and uploads `nit-x86_64-linux` with its checksum and a build provenance attestation. To confirm that a binary came from that workflow, run:

```sh
gh attestation verify nit-x86_64-linux --repo Telefragged/nit
```

The repository settings carry two rulesets:

- `main`: no direct push and no force push, the `nix` and `semver` jobs are required status checks, and a PR must be up to date with `main` before it merges. A repository owned by a personal account has no merge queue.
- Tags `*.*.*`: no update and no deletion. A ruleset cannot exempt the workflow token, so tag creation stays open for the release workflow.
