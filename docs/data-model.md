# Data model

SQLite, migrations applied at startup (`PRAGMA user_version`), WAL mode,
`busy_timeout` set. Review state only — git objects stay in the user's repo,
pinned where needed (see "GC safety"). **Nothing is ever hard-deleted**:
rows are status-flagged and every status is re-derivable by a later scan.

## Tables

```sql
repos     (id, path UNIQUE, created_at)
chains    (id, repo_id, branch, base, status, last_scan_error,
           created_at, updated_at, UNIQUE(repo_id, branch))
           -- status: active | merged | abandoned   (all re-derivable)
           -- last_scan_error: NULL or human-readable scan failure
changes   (id, chain_id, change_key, position, status,
           UNIQUE(chain_id, change_key))
           -- status: pending | approved | changes_requested | commented
           --         | orphaned
changes   -- position is NULL while orphaned
revisions (id, change_id, number, commit_sha, parent_sha, effective_tree,
           fixups, message, created_at, UNIQUE(change_id, number))
           -- number: 1-based patchset number
           -- effective_tree: tree sha with this change's fixups folded in
           --   (the commit's own tree when no fixups; NULL = fold conflict)
           -- fixups: JSON [{sha, message}] folded in, branch order
comments  (id, change_id, revision_number, parent_id, author, file, line,
           side, line_text, body, state, resolved, review_id,
           created_at, updated_at)
           -- author: reviewer | agent;  state: draft | published
           -- side: old | new;  line_text: snapshot of the anchored line
           -- parent_id: reply threading; resolved: thread-level bool
reviews   (id, change_id, revision_number, verdict, message, created_at)
           -- verdict: approve | request_changes | comment
events    (id, chain_id, kind, payload, created_at)
           -- id is the monotonic long-poll cursor
           -- kind: chain_updated | review_submitted | comment_replied
           --       | chain_closed
           -- payloads are internal (clients act on /wait's feedback
           -- snapshot, never on events): chain_updated {chain_id},
           -- chain_closed {chain_id, status}, review_submitted
           -- {chain_id, change_id, review_id, verdict}, comment_replied
           -- {chain_id, change_id, comment_id}
```

Scan warnings (duplicate Change-Id, squash! seen, …) are per-scan,
recomputed each rescan and held in memory only — they are not persisted.

## Concurrency (normative)

- One **per-chain async mutex** serializes every scan of a chain *and* every
  review submission to it. No revision insert, status flip, or 409 check
  happens outside it.
- Each scan / review-submit runs in **one transaction** (`BEGIN IMMEDIATE`).
- Scans are throttled: a scan that finished < 2s ago is not repeated; reads
  serve current DB state instead of waiting on a running scan.
- A failed scan **never** partially reconciles: the transaction rolls back,
  `chains.last_scan_error` is set, previous state stays served. One broken
  chain must not affect listing the others.

## Change identity (`change_key`)

A change keeps its identity while its commit sha changes (rebase, amend,
autosquash). Matching, in priority order:

1. **`Change-Id:` trailer** (gerrit-style, any opaque token). If two live
   commits in one chain carry the same trailer, the first (oldest) keeps it;
   later ones get derived keys (`I123#2`, …) and the scan records a warning
   surfaced in the push response and chain banner.
2. **Exact sha** — commit unchanged since last scan.
3. **Patch-id** (`git patch-id --stable` semantics; empty diffs use the
   sentinel patch-id of the empty string) — same diff, new sha.
4. **Subject** — first line matches an existing non-orphaned change whose
   commit left the branch.

Unmatched commits become new changes. Existing changes whose commit
disappears and matches nothing become **`orphaned`** — comments, drafts and
reviews are kept; the UI shows them collapsed. A later scan that finds a
matching commit again (rules above) re-attaches the orphan (status returns
to its pre-orphan value). Orphans are how transient git states (mid-rebase
resets, dropped-and-restored commits) and "split this commit" reworks stay
lossless.

## Scan algorithm (push + throttled on reads, always under the chain lock)

1. Open repo; resolve `base` and branch tip. Failures (repo moved, base
   gone) → set `last_scan_error`, keep prior state, done.
   - Branch ref missing: only after the ref is missing on **two consecutive
     scans ≥ 10s apart** → status `abandoned` + `chain_closed` event
     (protects against mid-rebase windows). The first observation is
     encoded as the branch-missing `last_scan_error` marker; repeat missing
     scans must not re-bump `chains.updated_at`, which times the window.
   - Merged test: tip is ancestor-or-equal of base **and** every live
     non-empty change's patch-id appears in `fork..base`, where *fork* is
     the recorded `parent_sha` of the first live change's latest revision
     (a plain merge-base would be empty after a ff-merge). The patch-id is
     taken over the **folded** diff (`parent_sha → effective_tree`) — that
     is what lands in base after autosquash-then-merge. Match → `merged` +
     `chain_closed`. (`tip == base` *without* the quorum is just an empty
     active chain — e.g. an agent's `reset --hard base` rebuild.)
   - A later scan that finds the branch alive with commits flips
     merged/abandoned back to `active`.
2. Walk `base..tip` oldest-first. **Any merge commit aborts the scan** with
   error "chain contains merge commits — rebase onto the base instead"
   (kept state + error surfaced, as above); a root commit in the range
   (unrelated history) aborts the same way. Split remaining commits into
   regular and fixup (`fixup! ` / `squash! ` subject prefix; `squash!` is
   folded like a fixup but adds a push warning since its message-editing
   semantics are interactive).
3. Attach each fixup to its target using **git autosquash semantics**
   (`todo_list_rearrange_squash`): among *earlier* commits, the **oldest**
   exact-subject match wins, else the remainder resolved as a commit-ish
   (sha prefix), else the oldest subject-prefix match — git's actual
   probe order. Fixups of fixups
   chain to the root target. A fixup with no target is a regular change.
   (Differential tests compare attachment against
   `git rebase -i --autosquash` todo output.)
4. Match regular commits to change rows (identity rules above); create new
   rows, update positions, orphan the unmatched, re-attach returning
   orphans.
5. Compute each change's **effective state**: `(commit_sha, [fixup shas])`.
   Effective tree = commit's tree with each fixup folded in by in-memory
   three-way merge (ancestor = fixup's parent tree, ours = accumulated
   tree, theirs = fixup tree). Conflict → `effective_tree = NULL`,
   `needs_rebase` reported on the change until the agent restructures.
6. If the effective state differs from the latest revision, insert revision
   `number+1`. Status effect:
   - fixup list unchanged **and** patch-id equal (pure rebase) → keep
     status, and review submission auto-retargets (see api.md);
   - anything else — including a patch-id-equal *new fixup* (the agent may
     be arguing in the fixup message) → status `pending`: the reviewer
     must look again.
7. Net structural difference → one `chain_updated` event.

A change's diff is always `parent_sha → effective_tree` of the selected
revision; earlier changes' fixups are *not* folded into later changes'
parents. Interdiff m→n is `effective_tree(m) → effective_tree(n)`.

## GC safety

Synthesized effective trees (and the fixup/commit objects review history
points at) must survive `git gc` and post-merge reflog expiry. After each
scan nit maintains one ref per revision of every change — orphans included,
their history must stay renderable — on active chains:
`refs/nit/keep/<chain-id>/<change-id>/<revision-number>` → a synthetic
commit whose tree is the effective tree and whose parents are
`[parent_sha's commit, original commit, each folded fixup commit]` (making
parent, original, fold *and* fixups reachable — the fixups are needed for
later pure-rebase comparisons and re-folds). Refs for merged/abandoned
chains are deleted by the scan
that closes them (review rows keep the shas; after that, history display is
best-effort). If a tree is missing anyway, the scan re-folds on demand.

## Status machine

```
change:  pending ──approve──▶ approved
         pending ──request_changes──▶ changes_requested
         pending ──comment──▶ commented        (reviewer asked/remarked)
         any ──new revision (per scan rule 6)──▶ pending
         any ──commit vanished──▶ orphaned ──reappears──▶ previous status

chain state (derived, not stored):
         any live change changes_requested | commented | needs_rebase
                                          → agents_turn
         else any live change pending     → waiting_for_review
         else all approved (≥1 change)    → ready_to_merge
         else (no live changes)           → agents_turn   (empty chain)

chain status (stored): active | merged | abandoned  — scan step 1; closed
chains leave the dashboard.
```

The full actionable/feedback contract lives in [api.md](api.md).
