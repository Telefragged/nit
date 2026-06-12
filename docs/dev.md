# Development

**Every command runs inside the devShell**: `nix develop -c <cmd>`, or let
direnv (`.envrc`) put you in it. Never call system cargo/node.

## Loops

```sh
# backend (auto-rebuild on change is fine via cargo-watch if added; plain:)
nix develop -c cargo run -- serve              # api on :8877
nix develop -c cargo test
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt

# frontend, in web/ — vite dev server on :5173 proxies /api to :8877
nix develop -c npm run dev                     # live UI w/ real backend
nix develop -c bash -c 'VITE_MOCK=1 npm run dev'   # UI with canned fixtures
nix develop -c npm run check                   # tsc
nix develop -c npm run build

# full production artifact
nix build                                      # → result/bin/nit
```

## Restarting the server

Rebuild (`nix build` or `cargo build`), ctrl-c the running `nit serve`
(in-flight `/wait` long-polls return immediately, so shutdown is prompt),
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
drafts, 409s, interdiff, needs_rebase…); live mode verifies real backend
data renders. Add a mock capture whenever you add a page or significant
state. Implementation lives in `web/screenshots/capture.mjs`; the npm
`@playwright/test` version must match `pkgs.playwright-driver` (the
devShell exports `$PLAYWRIGHT_DRIVER_VERSION`).

## Testing expectations

- Rust: unit tests next to the code; scan/identity logic gets real-git
  integration tests (`tempfile` + git2 building tiny repos). `cargo test`
  must stay green.
- Frontend: tsc-clean always; component logic that's easy to break (diff
  rendering, comment anchoring) deserves vitest tests if it grows hairy.
- End-to-end: `scripts/e2e.sh` drives the full agent↔reviewer loop against
  a fixture repo using the built binary.

## Commit & branch discipline

- Small commits, one concern each, imperative subject, body explains *why*.
- Never mix refactors with behavior changes.
- Parallel work happens in worktrees under `.worktrees/` on `track/*`
  branches; they land on `main` via rebase + fast-forward only. No merge
  commits anywhere.
- End commit messages with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
  when Claude wrote them.

## Landing changes — the nit review loop

This repo dogfoods itself: finished work is pushed as a nit chain and
reviewed by a human before it ff-merges to `main`. Agents drive the loop
with the `nit-review` skill (`.claude/skills/nit-review/SKILL.md`); the
underlying protocol is `docs/agent-workflow.md`.

### Review exemptions

Changes matching an entry here may land on `main` directly (same commit
discipline, still green):

- *(none yet — add bullets like "screenshot fixture data" or
  "typo-level doc fixes" as policy emerges)*

Ad-hoc exemption: the user saying "skip nit" / "land this directly" for a
specific change. When in doubt, review.
