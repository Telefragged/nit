# Development

This page has the rules for every change. Each area has its own page:

- `web.md`: the screenshot harness and the component gallery.
- `generated.md`: the generated files, and when to regenerate them.
- `releases.md`: the public contract, the release check, and releases.
- `design-review-guide.md`: the design anti-patterns to catch in a design
  review.

## Verification

Checks verify a change, not a green build — `nix build` skips tests
(crate2nix builds the crate with `runTests` off). Before every commit
(golden rule 9):

- `nix develop -c cargo check` — fast inner-loop gate.
- `nix flake check` — the pre-commit gate: builds the product and runs the
  Rust validators (`test`, `test-nit-types`, `test-nit-wasm`) and the web
  validators (`web-lint`, `web-test`, `web-screenshots`, `web-gallery`). Every crate of ours
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
  ```

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

   Write the PR title and body for a reader on GitHub. nit runs locally, so
   that reader cannot see its reviews, changes, or threads. Do not mention
   nit or the review in the PR. Describe what the chain changes and why.

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
