# Data model

SQLite, migrations applied at startup (`PRAGMA user_version`), WAL mode,
`busy_timeout` set. Review state only — git objects stay in the user's repo,
pinned where needed (see "GC safety"). **Nothing is ever hard-deleted**:
rows are status-flagged and every status is re-derivable by a later scan.

## Tables

```sql
repos     (id, path UNIQUE, created_at)
chains    (id, repo_id, branch, base, status, partial, last_scan_error,
           created_at, updated_at, UNIQUE(repo_id, branch))
           -- status: active | merged | abandoned   (all re-derivable)
           -- partial: sticky more-commits-coming bool; set/cleared only by
           --   registration (push --partial / ready), never by scans;
           --   a flip emits chain_updated but never bumps updated_at
           -- last_scan_error: NULL or human-readable scan failure
changes   (id, chain_id, change_key, position, status,
           UNIQUE(chain_id, change_key))
           -- status: pending | approved | changes_requested | commented
           --         | orphaned
changes   -- position is NULL while orphaned
revisions (id, change_id, number, commit_sha, parent_sha, message,
           created_at, UNIQUE(change_id, number))
           -- number: 1-based patchset number
comments  (id, change_id, revision_number, parent_id, author, file, line,
           side, range_start_line, range_start_char, range_end_line,
           range_end_char, line_text, body, state, resolved, review_id,
           created_at, updated_at)
           -- author: reviewer | agent;  state: draft | published
           -- side: old | new;  line_text: snapshot of the anchored line
           -- range_*: optional selected-text anchor (api.md "Range
           --   comments"); all four set or all NULL; range_end_line = line
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

## Concurrency (normative)

- One **per-chain async mutex** serializes every scan of a chain, every
  review submission to it, and every registration `partial` write. No revision
  insert, status flip, partial flip, or 409 check happens outside it.
- Each scan / review-submit / `partial` write runs in **one transaction**
  (`BEGIN IMMEDIATE`).
- Scans are throttled: a scan that finished < 2s ago is not repeated; reads
  serve current DB state instead of waiting on a running scan.
- A failed scan **never** partially reconciles: the transaction rolls back,
  `chains.last_scan_error` is set, previous state stays served. One broken
  chain must not affect listing the others.

## Change identity (`change_key`)

The **`Change-Id:` trailer** (gerrit-style, any opaque token) is the
identity, required and canonical: every commit in `base..tip` must carry
its own — a missing trailer, a token shared by two commits, or a
`fixup!`/`squash!` commit (squash locally before pushing) aborts the scan
with the violation in `last_scan_error`, kept state, like merge commits.

`change_key` = the trailer. A change keeps its identity — and its comment
history — while its commit sha changes (rebase, amend, reword); changing
a commit's Change-Id makes it a new change.

Commits with a new trailer become new changes. Existing changes whose
trailer disappears from the walk become **`orphaned`** — comments, drafts
and reviews are kept; the UI shows them collapsed. A later scan that sees
the trailer again re-attaches the orphan (status returns to its
pre-orphan value). Orphans are how transient git states (mid-rebase
resets, dropped-and-restored commits) stay lossless.

## Scan algorithm (push + throttled on reads, always under the chain lock)

1. Open repo; resolve `base` and branch tip. Failures (repo moved, base
   gone) → set `last_scan_error`, keep prior state, done.
   - Branch ref missing: only after the ref is missing on **two consecutive
     scans ≥ 10s apart** → status `abandoned` + `chain_closed` event
     (protects against mid-rebase windows). The first observation is
     encoded as the branch-missing `last_scan_error` marker; repeat missing
     scans must not re-bump `chains.updated_at`, which times the window.
   - Merged test: tip is ancestor-or-equal of base **and** every live
     change matches a commit in `fork..base`, where _fork_ is the recorded
     `parent_sha` of the first live change's latest revision (a plain
     merge-base would be empty after a ff-merge). A change matches by
     **Change-Id trailer first** — immune to the patch-id context drift
     that amending a _neighboring_ change causes — then by the
     patch-id of its diff (`parent_sha → commit tree`, what lands in base
     after rebase-then-merge); empty diffs are trivially
     matched but at least one real match is required. If no live changes
     exist (an earlier failed quorum orphaned them), the orphans are
     judged instead — reset-to-base rebuilds still can't match. Match →
     `merged` + `chain_closed`. (`tip == base` _without_ the quorum is
     just an empty active chain.)
   - A later scan that finds the branch alive with commits flips
     merged/abandoned back to `active`.
2. Walk `base..tip` oldest-first. **Any merge commit aborts the scan** with
   error "chain contains merge commits — rebase onto the base instead"
   (kept state + error surfaced, as above); a root commit in the range
   (unrelated history) aborts the same way. Then validate identity: every
   walked commit must carry its own `Change-Id:` trailer and must not be a
   `fixup!`/`squash!` commit — violations abort the same way ("Change
   identity" above).
3. Match commits to change rows by Change-Id key; create new
   rows, update positions, orphan the unmatched, re-attach returning
   orphans.
4. If a change's commit sha differs from its latest revision's, insert
   revision `number+1`. Status effect:
   - patch-id equal **and** commit message unchanged (pure rebase) → keep
     status, and review submission auto-retargets (see api.md);
   - anything else — including a message reword (the message is
     reviewable as `/COMMIT_MSG`) → status `pending`: the reviewer must
     look again.
5. Net structural difference → one `chain_updated` event.

A change's diff is always `parent_sha → commit tree` of the selected
revision. Interdiff m→n is `tree(m) → tree(n)`.

## GC safety

The commit objects review history points at must survive `git gc` and
post-merge reflog expiry. After each scan nit maintains one ref per
revision of every change — orphans included, their history must stay
renderable — on active chains:
`refs/nit/keep/<chain-id>/<change-id>/<revision-number>` → the revision's
commit (its parent, the diff's old side, is reachable through it). Refs
for merged/abandoned chains are deleted by the scan that closes them
(review rows keep the shas; after that, history display is best-effort).

## Status machine

```
change:  pending ──approve──▶ approved
         pending ──request_changes──▶ changes_requested
         pending ──comment──▶ commented        (reviewer asked/remarked)
         any ──new revision (per scan rule 4)──▶ pending
         any ──commit vanished──▶ orphaned ──reappears──▶ previous status

chain state (derived, not stored):
         any live change changes_requested | commented
                                          → agents_turn
         else any live change pending     → waiting_for_review
         else all approved (≥1 change)    → agents_turn if the chain is
                                            partial (still pushing), else
                                            ready_to_merge
         else (no live changes)           → agents_turn   (empty chain)

chain status (stored): active | merged | abandoned  — scan step 1; closed
chains leave the dashboard.
```

The full actionable/feedback contract lives in [api.md](api.md).
