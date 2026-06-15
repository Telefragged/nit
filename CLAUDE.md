# nit

Commit-level code review for AI coding agents: agents register a branch
(a "chain"), a human expert reviews each commit gerrit-style (draft line
comments, approve/request changes), agents resume on feedback and answer
by amending the reviewed commits in place — the required `Change-Id:`
trailer carries identity across rewrites. Product spec: `nit.md`.

## Golden rules

1. **All dev runs in the nix devShell** — `nix develop -c <cmd>`. Never use
   system toolchains. `nix build` must stay green.
2. **Small, single-purpose commits.** One concern per commit; no merge
   commits — an approved chain lands via this repo's approve action, a
   rebase + fast-forward only (docs/dev.md "The approve action").
3. **Every commit is treefmt-clean** — `nix develop -c treefmt` before
   each commit; after every rebase re-format each rewritten commit (not
   just the tip), above all after resolving merge conflicts — recipe in
   docs/dev.md "Formatting".
4. **Cross-component JSON shapes live in docs/api.md.** Change the doc
   first, then both sides (`crates/nit/src/api/types.rs`,
   `web/src/api/types.ts`).
5. **To see the UI, render it**:
   `cd web && nix develop -c npm run screenshots`, then Read
   `screenshots/*.png`.
6. **Changes land through nit itself** — start in a worktree
   (`.worktrees/<slug>` on a `track/<slug>` branch; the default for all
   work, not just parallel — recipe in docs/dev.md), then drive the review
   loop with the `nit-review` skill (`.claude/skills/nit-review/SKILL.md`).
   Direct-to-main only for ad-hoc user opt-outs and the exemptions listed
   in docs/dev.md.
7. **`nit push --partial` after every completed commit** — pushing is part
   of finishing a commit, like treefmt and the Change-Id, never a phase
   after the branch. No planned later step (cleanup, self-review,
   verification pass) delays a push: push now, amend later — amends become
   new revisions by design. An unpushed commit is invisible to the
   reviewer.
8. **Simplicity over caution — remove, then change, then add.** nit is
   pre-v1 and well-tested, so changes are cheap; do not hedge against blast
   radius. Reach for the simplest solution in that order of preference:
   delete code before rewriting it, rewrite existing code before adding
   new code. A large rewrite that ends up simpler beats a small diff that
   leaves complexity standing — if a simpler design exists, take it however
   much it touches. This governs review and simplification passes too
   (adversarial agents included): the status quo is never the "safe"
   default; reject a change only when it is not actually simpler or it
   breaks behavior, never because it changes a lot.
9. **Checks are verification — `cargo check` is the floor.** A commit is
   done only when `nix develop -c cargo check` passes and the flake
   validators are green: `nix flake check` runs `clippy` (`-D warnings`)
   and the full test suite as crane checks. `nix build` builds the product
   without running tests (rule 1), so a green build is necessary but not
   sufficient — run the checks before every commit (docs/dev.md
   "Verification").

## Layout

- `crates/nit/` — Rust: axum server, git2 scanning, sqlite state, CLI
- `web/` — React/TS SPA (Vite)
- `docs/` — read the one you need:
  - `architecture.md` — components, dataflow, key decisions — **start here**
  - `data-model.md` — schema, change identity, scan algorithm, status machine
  - `api.md` — HTTP/JSON contract (source of truth)
  - `frontend.md` — pages, design language, mock mode
  - `agent-workflow.md` — how coding agents drive nit (push/wait loop)
  - `dev.md` — dev loops, screenshot harness, testing, commit discipline
