# nit

Commit-level code review for AI coding agents: agents register a branch (a
"chain"), a human reviews each commit gerrit-style (draft line comments,
approve/request changes), agents resume on feedback and answer by amending
the reviewed commits in place — the required `Change-Id:` trailer carries
identity across rewrites. Product spec: `nit.md`.

## Golden rules

1. **All dev runs in the nix devShell** — `nix develop -c <cmd>`, never
   system toolchains. `nix build` stays green.
2. **Small, single-purpose commits**, one concern each, with the message
   **hard-wrapped at 72 columns** — a one-line subject, then a body of
   72-column-wrapped prose, never a single long line (docs/dev.md "Commit &
   branch discipline"). No merge commits — an approved chain lands via the
   approve action, rebase + fast-forward only (docs/dev.md "The approve
   action").
3. **Every commit treefmt-clean** — `nix develop -c treefmt` before each
   commit, and re-format every rewritten commit after a rebase (not just
   the tip), especially after conflict resolution (docs/dev.md
   "Formatting").
4. **Cross-component JSON shapes live in docs/api.md** — change the doc
   first, then both sides (`crates/nit/src/api/types.rs`,
   `web/src/api/types.ts`).
5. **To see the UI, render it**:
   `cd web && nix develop -c npm run screenshots`, then Read
   `screenshots/*.png`.
6. **Changes land through nit itself — the default for _every_ change
   unless the user says otherwise.** Start in a worktree (`.worktrees/<slug>`
   on a `track/<slug>` branch), then drive the review loop with the
   `nit-review` skill (`.claude/skills/nit-review/SKILL.md`). Size,
   triviality, or "it's self-contained" never lower the bar — a one-line
   docs fix takes the same path as a feature. Direct-to-main requires an
   explicit, up-front "skip nit" / "land directly" from the user (or a
   docs/dev.md exemption); it is never a call you make yourself, and never a
   label you apply after the fact to work already started on `main`. When in
   doubt, worktree.
7. **`nit push --partial` after every completed commit** — pushing
   finishes a commit, like treefmt and the Change-Id; no later pass
   (cleanup, self-review, verification) delays it. Push now, amend later —
   amends become new revisions by design. An unpushed commit is invisible
   to the reviewer.
8. **Simplicity over caution — remove, then change, then add.** nit is
   pre-v1 and well-tested, so changes are cheap; don't hedge against blast
   radius. Prefer deleting code to rewriting it, and rewriting to adding; a
   large rewrite that ends up simpler beats a small diff that leaves
   complexity standing. This binds review and simplification passes too
   (adversarial agents included): reject a change only when it isn't
   actually simpler or it breaks behavior, never because it changes a lot.
9. **Checks are verification — `cargo check` is the floor.** A commit is
   done when `nix develop -c cargo check` passes and the flake validators
   are green: `nix flake check` runs `clippy` (`-D warnings`) and the full
   test suite as crane checks. A green `nix build` is necessary but not
   sufficient — it skips tests (docs/dev.md "Verification").

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
