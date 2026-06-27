# Development

**Every command runs in the devShell**: `nix develop -c <cmd>` (or direnv
via `.envrc`). Never use system cargo/node.

## Loops

```sh
# backend
nix develop -c cargo run -- serve              # api on :8877
nix develop -c cargo check                     # fast compile gate
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
nix develop -c cargo fmt

# frontend, in web/ — vite dev server on :5173 proxies /api to :8877
nix develop -c npm run dev                     # live UI w/ real backend
nix develop -c bash -c 'VITE_MOCK=1 npm run dev'   # UI with canned fixtures
nix develop -c npm run check                   # tsc
nix develop -c npm run lint                     # eslint + stylelint
nix develop -c npm run build

# product artifact + validators
nix build                                      # product only → result/bin/nit (no tests)
nix flake check                                # build + clippy + test validators
```

Changing `web/package-lock.json` means refreshing `npmDepsHash` in
`flake.nix` (`nix run nixpkgs#prefetch-npm-deps -- web/package-lock.json`
prints it); a stale hash breaks `nix build` and every
`nix run '…?ref=main#nit'` CLI invocation.

## Verification

Checks verify a change, not a green build — `nix build` skips tests
(`doCheck = false`). Before every commit (golden rule 9):

- `nix develop -c cargo check` — fast inner-loop gate.
- `nix flake check` — the pre-commit gate: builds the product and runs the
  `clippy` (`-D warnings`) and `test` crane validators. Run one alone with
  `nix build .#checks.<system>.clippy` or `.#checks.<system>.test`.

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

## Linting

`nix develop -c npm run lint` (in `web/`) runs ESLint then Stylelint, the
frontend counterpart to backend `clippy::pedantic`; both stay green.
Formatting is prettier's job, not theirs. Config: `web/eslint.config.js`,
`web/stylelint.config.js`.

Strictness only ratchets **up**. Suppress a lint only with a reason —
`#[expect(..., reason = "…")]` on the backend (`#[allow]` is denied), a
reasoned inline disable on the frontend (a bare one is flagged stale). The
backend also denies `unwrap` (use `expect` with a reason), via the
workspace lints in the root `Cargo.toml`. The frontend allow-list of
not-yet-satisfied rules only shrinks: re-enable a rule by removing its line
and fixing the code in the same commit.

Relax a rule only when the fix it forces is uglier than the risk it guards
(e.g. allowing `${n}` number interpolation over wrapping every span in
`String()`) — and say why. "It's a lot of edits" is not a reason.

## Type discipline — let the types make illegal states unrepresentable

Lean on the type system instead of runtime checks or convention. Three
rules, binding on review and simplification passes too (a violation is a
finding to fix, and reviewer agents are told so):

- **A closed set of values is an `enum`, never a `String`** (sides,
  verdicts, statuses, kinds…). Home: `crates/nit/src/enums.rs`, mirrored by
  the TS unions in `web/src/api/types.ts`. `#[serde(rename_all = …)]` keeps
  the wire spelling, so it is not a wire change. Buys exhaustive `match`es
  and a 400 on an unknown value at deserialize time. A `String` is fine
  only at the storage boundary, converted to the enum immediately.
- **Absence is not a state — model it.** Encode the legal combinations of a
  cluster of `Option`s as an enum so the illegal ones can't be built: a
  thread's location is `review::Anchor` (`Change | File | Line { … }`), not
  five loose `Option`s.
- **One input names one thing.** Identify a thing two ways with two
  type-distinct flags, not one that sniffs the value's form: `nit comment`
  takes `--change <u64>` or `--change-id <String>`, never one flag that
  guesses.

## Restarting the server

Rebuild, ctrl-c the running `nit serve`, restart with the same `--db`.
Parked `nit wait`s ride it out (backoff retry, persisted cursor — no events
missed); `push`/`status`/`reply` during the gap fail fast, so rerun them.

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
`pkgs.playwright-driver` (the devShell sets `$PLAYWRIGHT_DRIVER_VERSION`).

## Testing expectations

- Rust: unit tests beside the code; scan/identity logic gets real-git
  integration tests (`tempfile` + git2). `cargo test` runs as the `test`
  flake check.
- Frontend: tsc-clean always; test break-prone logic (diff rendering,
  comment anchoring) with vitest (`npm test`).
- End-to-end: `scripts/e2e.sh` drives the full loop against a fixture repo.
- A fresh `.worktrees/*` checkout has no `web/node_modules`; run
  `cd web && nix develop -c npm ci` before any web check.

## Commit & branch discipline

- Small commits, one concern each.
- **Hard-wrap the commit message at 72 columns.** This is not optional and is
  checked in review. The subject is a single line stating the _what_ (for
  indexing); after a blank line, the body explains _why_ as 72-column-wrapped
  prose — each line broken at ≤72 like a paragraph, **never one long line you
  let the terminal soft-wrap**. With `git commit`, write the body across real
  newlines (a `-m` per paragraph, lines pre-wrapped), not a single sentence.
- Keep messages **timeless** — no process narration ("rebased onto X", branch
  ordering); that goes in the `nit` reply or terminal, not git history.
- Code comments are stricter: a comment says what the code **is**, never
  how it got there (no "now / no longer / replaced"). git blame holds the
  history.
- Every commit treefmt-clean (re-format after a rebase — "Formatting").
- Never mix refactors with behavior changes.
- **Every change starts in its own worktree** on a `track/*` branch — the
  default for _all_ work unless the user has explicitly said otherwise — so
  `main` stays put and chains never serialize on a shared branch:

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
the approve action lands it on `main`. Drive the loop with the
`nit:lifecycle` skill; protocol in `docs/agent-workflow.md`. Run the `nit`
CLI from the build that matches the running server (normally `main`'s: `nit`
on PATH, else `nix run '…?ref=main#nit'`), not your branch's binary.

### The approve action

nit derives `approved` (every live change approved)
but doesn't prescribe landing — each project defines that. Here it's a
fast-forward-only merge to `main` (no merge commits — golden rule 2). The
agent that built the chain **drives it all the way to `merged`**: reaching
`approved` is the cue to land, never to hand off.

```sh
# if main moved: rebase onto it, verifying each replayed commit
git rebase -x 'nix flake check' main
nit push                                       # re-record the rebased revisions (see below)
git -C <primary> merge --ff-only <branch>      # never `git checkout main` inside a worktree
git worktree remove <worktree> && git branch -d <branch>
```

The lifecycle timer marks the chain `merged` once the commits are on `main`,
matching each change's latest revision patch-id against the canonical branch
— so the `nit push` after the rebase matters: it re-records each revision at
its rebased sha. A pure rebase keeps its patch-id (and its approval), so the
push just adds a no-op revision; a rebase that **resolved conflicts** changed
those patch-ids, and without the push the timer never matches them, so they
never flip to `merged`. Don't push _after_ the ff-merge — the tip is on
`main` then, an empty walk, a 409.

Landing is the agent's responsibility **all the way to `merged`; you stop
only when it is fundamentally impossible** — an unresolvable rebase, or you
genuinely cannot write to `main`. `--ff-only` keeps `main` linear; a non-ff
branch is a rebase to do, not a reason to pause. `main` moving under you
(another agent landing in parallel) is the same: rebase onto it and land —
coordinate, never abandon the chain. A conflict-resolving rebase that resets
a change to `pending` is still yours to land; the content was approved and
the resolution is mechanical.

### Review exemptions

**The default is unconditional: unless the user has said otherwise, every
change runs through nit.** Start it in a worktree and drive the review loop —
regardless of size, triviality, or whether it "looks self-contained." This is
not an agent judgement call; a one-line docs fix takes the same path as a
feature.

A change may land on `main` directly **only** with an explicit, up-front
instruction for that change — the user saying "skip nit" / "land directly" —
or under a standing entry in the list below. Absent that, route through nit.
You do not get to reclassify a change as exempt after the fact, and "I already
edited `main`" is never a justification — move it to a worktree. When in
doubt, review.

Standing exemptions (same discipline, still green):

- _(none yet)_
