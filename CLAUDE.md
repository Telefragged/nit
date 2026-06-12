# nit

Commit-level code review for AI coding agents: agents register a branch
(a "chain"), a human expert reviews each commit gerrit-style (draft line
comments, approve/request changes), agents resume on feedback and answer
with `fixup!` commits. Product spec: `nit.md`.

## Golden rules

1. **All dev runs in the nix devShell** — `nix develop -c <cmd>`. Never use
   system toolchains. `nix build` must stay green.
2. **Small, single-purpose commits.** One concern per commit; no merge
   commits — worktrees land via rebase + fast-forward (see docs/dev.md).
3. **Cross-component JSON shapes live in docs/api.md.** Change the doc
   first, then both sides (`crates/nit/src/api/types.rs`,
   `web/src/api/types.ts`).
4. **To see the UI, render it**: `cd web && nix develop -c npm run
   screenshots`, then Read `screenshots/*.png`.
5. **Changes land through nit itself** — branch, then drive the review
   loop with the `nit-review` skill (`.claude/skills/nit-review/SKILL.md`).
   Direct-to-main only for ad-hoc user opt-outs and the exemptions listed
   in docs/dev.md.

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
