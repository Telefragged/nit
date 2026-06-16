# Development

**Every command runs inside the devShell**: `nix develop -c <cmd>`, or let
direnv (`.envrc`) put you in it. Never call system cargo/node.

## Loops

```sh
# backend (auto-rebuild on change is fine via cargo-watch if added; plain:)
nix develop -c cargo run -- serve              # api on :8877
nix develop -c cargo check                     # fast compile gate — run it often
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

`nix build` pins the web dependencies by hash: any commit that changes
`web/package-lock.json` must also refresh `npmDepsHash` in `flake.nix`
(`nix run nixpkgs#prefetch-npm-deps -- web/package-lock.json` prints the
new value) and verify `nix build`. A stale hash breaks `nix build` — and
with it every `nix run 'git+file://…?ref=main#nit'` CLI invocation.

## Verification

A change is verified by its checks, not by a successful build. `nix build`
produces the product binary only — tests do **not** run as part of it
(`doCheck = false`). Run the validators before every commit (golden
rule 9):

- `nix develop -c cargo check` — the fast inner-loop gate; run it often
  while editing.
- `nix flake check` — the authoritative pre-commit gate. It builds the
  product and runs the crane validators: `clippy`
  (`--all-targets -- -D warnings`, the same lints as the devShell command)
  and `test` (the full `cargo test` suite, with the git identity the
  differential rebase test needs). Build one in isolation with
  `nix build .#checks.<system>.clippy` or `.#checks.<system>.test`.

A commit is not done until `cargo check` and the flake checks are green.

## Formatting

`nix develop -c treefmt` formats the whole tree (`nix fmt` runs the
same thing) — config in `treefmt.toml`, formatter binaries pinned by
the devShell (rustfmt, prettier, nixfmt, shfmt, taplo).
`treefmt --fail-on-change` is the check form.

Formatting is **per commit**, not per branch: run treefmt before every
`git commit`, so each commit is treefmt-clean on its own, parallel
chains never conflict on whitespace, and review interdiffs show real
changes only. Committing or amending from a formatted tree keeps that
invariant; a **rebase does not** — replayed commits and hand-typed
conflict resolutions land unformatted in whichever commit they touched,
where amending the tip cannot reach them. So after every rebase,
re-format each rewritten commit in place:

```sh
git rebase -x 'nix develop -c treefmt && if ! git diff --quiet; then git commit -a --amend --no-edit; fi' \
  "$(git merge-base main HEAD)"    # when landing: onto main instead
```

No-op on a clean chain. Check-only form: exec
`nix develop -c treefmt --fail-on-change` instead — it stops at the
first unformatted commit.

## Linting

`nix develop -c npm run lint` (in `web/`) runs ESLint then Stylelint —
the frontend counterpart to `clippy::pedantic` on the backend. ESLint
covers `.ts`/`.tsx`/`.html` with strict, type-aware rules
(typescript-eslint strict + stylistic, react/hooks, jsx-a11y,
@html-eslint); Stylelint covers `.css` with stylelint-config-standard
(`npm run lint:css` for CSS alone). Both must stay green, same as clippy.
Formatting is **not** their job: `eslint-config-prettier` and
stylelint-config-standard both defer whitespace to prettier (run via
treefmt), so lint and format never fight.

Config is `web/eslint.config.js` and `web/stylelint.config.js`. Disables
come in exactly two kinds: formatter-owned (permanent — prettier's
territory) and a burn-down allow-list (temporary — rules the strict
presets enable that the code doesn't satisfy yet, each silenced with its
remaining count). The allow-list only shrinks: a new agent's first output
is held to every rule not on it, and the list is whittled to empty over
time. Re-enabling a rule means removing its line **and** fixing the code
in the same commit, never relaxing it back. A genuinely ill-fitting rule
gets a reasoned inline disable instead (ESLint's
`reportUnusedDisableDirectives` flags it when stale — the `#[expect]`
model), never a silent permanent allow.

## Type discipline — let the types make illegal states unrepresentable

Lean on the type system; do not push validation that a type could do into
runtime checks or convention. Two standing rules:

- **A closed set of values is an `enum`, never a `String`.** Every field
  that can only be one of a fixed list (sides, verdicts, statuses, kinds,
  authors, …) is a serde enum, not a string. The Rust home for these is
  `crates/nit/src/enums.rs`; the TS mirror is the union types in
  `web/src/api/types.ts`. A `#[serde(rename_all = "snake_case")]` (or
  per-variant `rename`) reproduces the wire spelling, so swapping a
  `String` for the enum is **not** a wire change — the JSON in `docs/api.md`
  is unchanged. The payoff is exhaustive `match`es (no `_ =>`
  fallthrough), no `as_str`/`from_str` round-tripping inside the server,
  and automatic rejection of an unknown value at deserialize time (a 400
  through `AppJson`) instead of a bad string flowing deeper. The one place
  a string is still acceptable is the storage boundary — a `TEXT` column or
  the dispatch on a not-yet-parsed log row — and it is converted to the
  enum immediately (`db::col_side`, `Entry::from_row`).

- **Absence is not a state — model it.** When a cluster of `Option` fields
  has only a few legal combinations, encode the combinations as an enum so
  the illegal ones are unrepresentable, rather than leaving the invariants
  to a doc comment. A thread's location is `review::Anchor`
  (`Change | File | Line { … }`), not five independent `Option`s where a
  `range` without a `line` or a `line` without a `file` could be built.

Both rules bind **review and simplification passes too** (the agents in a
two-pronged review included): a new stringly-typed enumerated field, or a
new bag of `Option`s standing in for an enum, is a finding to fix, never
the accepted baseline — and a reviewer agent is told so explicitly when it
is spawned.

## Restarting the server

Rebuild (`nix build` or `cargo build`), ctrl-c the running `nit serve`
(in-flight `/events` streams end on the shutdown signal, so shutdown is prompt),
then start it again with the same `--db`. Parked `nit wait`s are
unaffected: each prints one stderr notice, retries with backoff (1–10s)
until the server is back, and resumes the same sqlite-persisted cursor —
no review events are missed. `nit push`/`status`/`reply` issued during
the gap fail fast ("is 'nit serve' running?"); just rerun them.

## Screenshot harness (frontend checking for AI agents)

AI agents can't look at a browser; they look at PNGs. Both modes write to
`screenshots/*.png` (repo root, gitignored) — to "see" the app, run one and
`Read` the PNGs:

```sh
# mock mode — every UI state from canned fixtures, no backend needed
cd web && nix develop -c npm run screenshots

# live mode — seeds a demo repo, runs the real nix-built server + UI
nix develop -c scripts/screenshots-live.sh     # needs ./result from nix build
```

Mock mode is the design-review workhorse (it covers detailed states:
drafts, 409s, interdiff…); live mode verifies real backend
data renders. Add a mock capture whenever you add a page or significant
state. Implementation lives in `web/screenshots/capture.mjs`; the npm
`@playwright/test` version must match `pkgs.playwright-driver` (the
devShell exports `$PLAYWRIGHT_DRIVER_VERSION`).

## Testing expectations

- Rust: unit tests next to the code; scan/identity logic gets real-git
  integration tests (`tempfile` + git2 building tiny repos). `cargo test`
  must stay green — it runs as the `test` flake check, not as part of
  `nix build` ("Verification" above).
- Frontend: tsc-clean always; component logic that's easy to break (diff
  rendering, comment anchoring) deserves vitest tests — `npm test` runs
  them (jsdom + testing-library, colocated `*.test.ts(x)` under `src/`)
  and must stay green.
- End-to-end: `scripts/e2e.sh` drives the full agent↔reviewer loop against
  a fixture repo using the built binary.

## Commit & branch discipline

- Small commits, one concern each. The imperative subject states the
  _what_ (for easy indexing); the body explains _why_ — the what is
  evident from the diff, so the why is the part worth writing. Hard-wrap
  the body at 72 columns. Keep the message **timeless**: read years later
  against the tree it produced, it carries no process narration — no
  "rebased onto X", "the harness landed meanwhile", branch ordering, or
  similar. That context is useful live; put it in the `nit` reply or
  terminal output, not the git history.
- Code comments are timeless too, and stricter: a commit _message_ may
  describe what the commit changes, but a code _comment_ may not. A
  comment states what the current code **is**, never how it got there —
  no "now / no longer / replaced the old / used to"; one that only
  narrates a change or a removed approach gets deleted, since git blame
  holds the history.
- Every commit treefmt-clean: format before committing, re-format and
  amend after any rebase or conflict resolution ("Formatting" above).
- Never mix refactors with behavior changes.
- **Every change starts in its own worktree** under `.worktrees/` on a
  `track/*` branch — the default for solo work, not just parallel work, so
  the main checkout stays on `main` and chains never serialize on a shared
  branch. Create one with:

  ```sh
  git worktree add .worktrees/<slug> -b track/<slug> main
  ```

  Commit there, drive the nit review loop from that worktree, and land via
  the approve action — rebase + fast-forward only, never a merge commit
  anywhere (recipe: "The approve action" below). Tear it down after
  landing: `git worktree remove .worktrees/<slug>` then
  `git branch -d track/<slug>`.

## Landing changes — the nit review loop

This repo dogfoods itself: finished work is pushed as a nit chain and
reviewed by a human before the approve action lands it on `main`. Agents
drive the loop with the `nit-review` skill
(`.claude/skills/nit-review/SKILL.md`); the underlying protocol is
`docs/agent-workflow.md`.

### The approve action

nit derives the `approved` state (every live change approved, chain not
`partial`) but does **not** prescribe what landing it means — that is the
**approve action**, defined per project. For this repo the approve action
is a fast-forward-only merge to `main` (no merge commits — golden rule 2):

```sh
# when main moved: rebase onto it, keeping every replayed commit
# treefmt-clean ("Formatting" above)
git rebase -x 'nix develop -c treefmt && if ! git diff --quiet; then git commit -a --amend --no-edit; fi' main
git checkout main && git merge --ff-only <branch>
nit push --repo <worktree> --branch <branch>   # scan flags the chain merged
git branch -d <branch>
```

Order matters: the scan must see the merge while the branch ref still
exists, so it records `merged`, not `abandoned`.

**Make the best effort to fully close an approved chain — don't stop at
`approved`.** `--ff-only` is there to keep `main` linear (no merge
commits), **not** to gate the work: a branch that isn't fast-forwardable
because `main` moved is a rebase to do, not a reason to pause. So the
approve action is always _rebase if needed, then `--ff-only` merge_, run
end to end — never pause to ask whether to land an approved chain; land
it.

In a worktree (`.worktrees/*`): rebase there, but never `git checkout
main` — main is checked out in the primary worktree. Run the merge from
that checkout: `git -C <primary-checkout> merge --ff-only <branch>`. The
only reason to stop at `approved` is a genuine ownership conflict —
another agent actively driving that checkout; absent that, close the
chain.

### Review exemptions

Changes matching an entry here may land on `main` directly (same commit
discipline, still green):

- _(none yet — add bullets like "screenshot fixture data" or
  "typo-level doc fixes" as policy emerges)_

Ad-hoc exemption: the user saying "skip nit" / "land this directly" for a
specific change. When in doubt, review.
