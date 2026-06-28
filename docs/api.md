# HTTP API — the contract

Everything under `/api`, JSON in/out. **This doc set is the single source of
truth for shapes**: the backend models them in `crates/nit-types`, from which
the frontend's `web/src/api/types.gen.ts` is generated (`nix run
.#gen-types`). Change a shape in its `api/` section file first.

Errors: non-2xx with `{"error": "human readable message"}`.
Times are RFC3339 strings. Shas are full 40-hex; clients truncate for
display (12 chars, the canonical short form).

The unit of state is the **change** (a `Change-Id`, scoped to a repo): it
owns an append-only log whose fold is its reviewable state
(docs/data-model.md). A **chain** is never stored — it is derived on demand
by walking a tip commit back to the repo's canonical branch through each
revision's recorded `parent_sha` (gerrit relation chains). Every read shape
below is served from the in-memory fold; chains are assembled at read time
from the per-change folds. Concurrency guarantees: docs/data-model.md
("Concurrency", normative).

A follower watches a **set** of changes over **one websocket** (`WS
/api/stream`, "Events"); one-shot reads (`nit status`, `nit log`) and the web
poll the same folds.

## Sections

- [Health](api/health.md)
- [Repos](api/repos.md)
- [Push](api/push.md)
- [Chains](api/chains.md)
  - [The B-in-two-chains example](api/chains.md#the-b-in-two-chains-example)
- [Graph](api/graph.md)
- [Changes](api/changes.md)
  - [The commit message as a file](api/changes.md#the-commit-message-as-a-file)
  - [Rebase-aware interdiffs](api/changes.md#rebase-aware-interdiffs)
  - [Comment placement](api/changes.md#comment-placement)
  - [Range comments](api/changes.md#range-comments)
- [Comments (drafts → published) — reviewer side](api/comments.md)
  - [Thread resolution](api/comments.md#thread-resolution)
- [Reviewer decisions (drafts)](api/decisions.md)
- [Agent endpoints](api/agent.md)
- [Events](api/events.md)
  - [The cursor](api/events.md#the-cursor)
  - [State table (normative)](api/events.md#state-table-normative)
- [Static UI](api/static.md)
