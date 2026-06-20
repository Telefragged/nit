## Chains

A chain is the ordered path from the canonical branch up to a tip commit,
each member pinned to the patchset that tip walked through. Nothing about a
chain is stored: these endpoints compute it from the in-memory tip-commit set
and the commit-sha → `(change, revision)` index (docs/data-model.md "Chain
derivation").

- `GET /api/chains?repo={id}` → `{"chains": [ChainSummary]}` — one entry per
  known **tip commit** (the dashboard). `status` defaults to `active` (live
  tips — neither merged nor abandoned, so an abandoned tip is hidden here);
  `all` also includes merged and abandoned tips.
- `GET /api/chains/{change_id}` → Chain — the derived path through that
  change's tip commit. An **abandoned** change still resolves (it stays a
  member, and a tip if it is a leaf) — abandonment is membership-inert.
  `?revision={n}` selects which patchset of the change to root on (default: its
  latest); the selected revision's `parent_sha` determines the path, so
  `?revision` _is_ the choice of chain context. 404 if the change is unknown.
- `GET /api/chains/{change_id}/log` → the **aggregated** chain log: every
  member's log entries, merged and sorted by global `seq` (one timeline for
  the whole chain). Behind `nit log`.
- `POST /api/chains/{change_id}/submit` → BatchSubmitResult — **publish every
  member's staged decision** for this chain (docs/data-model.md "Reviewer
  decisions"). `?revision={n}` picks the chain context exactly like
  `GET /api/chains/{change_id}` (default: the change's latest). The path is
  re-derived at submit time; for each member carrying a staged decision it
  publishes that decision **at the revision this path pins on the member** (not
  a stored revision), in that member's own per-change transaction (atomic per
  change, **not** atomic across the chain — like `nit push`). A member with no
  staged decision is left untouched (its comment drafts stay drafts). The
  per-change publish deletes the member's `draft_reviews` row, so re-submitting
  finishes a torn batch without double-publishing — submit is idempotent.

  ```json
  resp: BatchSubmitResult = {
    "submitted": 2,                       // members whose decision published
    "errors": [SubmitError]               // members skipped (stale/terminal)
  }
  SubmitError = {"change_id": 11, "message": "change is abandoned — stage Reopen"}
  ```

```json
ChainSummary = {
  "tip_change_id": 12,
  "repo_id": 1,                  // the repo this chain belongs to
  "name": "feat/x",              // best-effort, resolved at query time (below)
  "state": "waiting_for_review", // derived — see state table
  "partial": false,              // the tip's latest revision is partial
  "updated_at": "…",             // newest member entry's time
  "path": [PathEntry]            // oldest-first, base → tip
}
Chain = {
  "tip_change_id": 12,
  "repo_id": 1,
  "name": "feat/x",
  "base_branch": "main",
  "state": "waiting_for_review",
  "partial": false,
  "path": [PathEntry]
}
PathEntry = {
  "change_id": 10, "position": 0,    // position is a property of THIS path
  "change_key": "I3f2…",
  "revision": 2,                     // the patchset this path walks
  "latest_revision": 3,              // newest patchset anywhere; > revision drives
                                     // the client's "newer elsewhere" badge
  "status": "pending",               // per (change, this revision)
  "merged_elsewhere": false,         // a newer revision landed on the canonical branch
  "subject": "server: add health endpoint",
  "commit_sha": "…",
  "counts": {"threads": 3, "drafts": 1, "unresolved": 2}, // scoped to this revision
  "draft_decision": "approve"        // the change's staged decision (Decision),
                                     // or null; change-wide (one per change),
                                     // so it shows on every chain the change is in
}
```

`position`, `status`, `unresolved`, and `state` are read **at the path's
pinned revision** — two tips placing the same change differently carry
independent verdicts (a request_changes in one chain never overwrites an
approve in another). `id` on a change is its stable fold id (the `change`
rowid); thread ids are fold-assigned by fold order (docs/data-model.md
"Identity").

`draft_decision` is the one exception to "read at the pinned revision": a draft
decision is **change-wide** (one per change), so the same value appears on every
chain the change is a member of. The dashboard counts a member as having
reviewer draft state when `counts.drafts > 0` **or** `draft_decision != null`,
and enables a chain's batch submit when any member carries a `draft_decision`.

### Tip names

A tip is named best-effort at query time (nit stores no branch key): a branch
ref that `git branch --contains <tip>` keeps stable as the agent advances,
else a tag, else the commit subject. A push that advances a tip keeps the same
name; deleting a branch only drops a name, not the tip.

### The B-in-two-chains example

Two pushes in one repo, canonical `main` at merge-base `m`:

- push 1: `m → A → B → C` (Change-Ids `Ia, Ib, Ic`)
- push 2: `m → D → B′ → E` (`Id, Ib, Ie`, B re-parented onto D)

`B` is one change with two patchsets: rev0 `parent=A`, rev1 `parent=D`. Two
tips, two chains: `chains/Ic` walks B at rev0, `chains/Ie` walks B at rev1.
Threads and reviews on B are **shared** (they belong to the change) and each
is anchored to the revision it was written against; `?revision` selects which
patchset — and chain context — you view.
