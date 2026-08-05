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
  "$(git merge-base main HEAD)"    # when landing: onto main instead
```

Two edges: don't run a bare treefmt before amending a checked-out historic
commit mid-rebase (it folds later commits' formatting into the amend —
stage only your files); and keep inline code spans on one line, or prettier
drops the hanging indent of a wrapped markdown list item.

## Screenshot harness (frontend checking for AI agents)

Agents read PNGs, not browsers. Both modes write `screenshots/*.png` (repo
root, gitignored); run one and `Read` them:

```sh
# mock mode — every UI state from fixtures, no backend
cd web && nix develop -c npm run screenshots
# live mode — real nix-built server + UI (needs ./result from nix build)
nix develop -c scripts/screenshots-live.sh
```

Mock mode covers detailed states (drafts, 409s, interdiff); add a capture
with every new page or state. The npm `@playwright/test` version must match
`pkgs.playwright-driver` (the devShell sets `$PLAYWRIGHT_DRIVER_VERSION`) —
the `web-screenshots` flake check runs mock mode in the sandbox and its
output _is_ the PNGs, so `nix build .#checks.<system>.web-screenshots` both
catches a skew and gives you the gallery in `result/`.

## Testing expectations

- Rust: unit tests beside the code; scan/identity logic gets real-git
  integration tests (`tempfile` + git2). `cargo test` runs as the `test`
  flake check.
- Frontend: tsc-clean always; test break-prone logic (diff rendering,
  comment anchoring) with vitest (`npm test`), which runs as the `web-test`
  flake check.
- The sidebar file tree renders into a shadow root, so `screen` queries and
  `getByTitle` never reach its rows: query
  `document.querySelector("file-tree-container").shadowRoot` for
  `[data-item-path="…"]`, and await its repaints (they land off-cycle).
- End-to-end: `scripts/e2e.sh` drives the full loop against a fixture repo.
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
  ```

  Address the worktree explicitly — absolute paths,
  `cargo --manifest-path <worktree>/crates/nit/Cargo.toml`,
  `git -C <worktree>` — never an ambient cwd, which may have drifted back
  to the primary checkout. Commit there, drive the review loop, land via
  the approve action, then `git worktree remove .worktrees/<slug>` and
  `git branch -d track/<slug>`.

- **Parallel chains stay independent**: never pre-merge in-flight branches
  into a shared integration branch — each is built and reviewed on its own,
  conflicts resolved only as each lands. (Rebasing one in-flight branch
  onto a moved `main` is fine.)

## Landing changes — the nit review loop

This repo dogfoods nit: push finished work as a chain, a human reviews it,
and the approve action — `nix develop -c scripts/land.sh`, the `land`
skill — lands it on `main` (rebase if `main` moved, `nix flake check` on
every commit, fast-forward merge; the lifecycle timer then marks the chain
`merged`). Drive the loop with the `nit:lifecycle` skill, all the way to
`merged`. Run the `nit` CLI from the build that matches the running server
(normally `main`'s: `nit` on PATH, else `nix run '…?ref=main#nit'`), not
your branch's binary.

### Review exemptions

Golden rule 6 is the default: every change runs through nit, in a worktree,
regardless of size. The only exits are an explicit, up-front "skip nit" /
"land directly" from the user for that change, or a standing entry below.

Standing exemptions (same discipline, still green):

- _(none yet)_
