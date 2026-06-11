# Data model

SQLite, migrations applied at startup (`PRAGMA user_version`). Review state
only — git objects stay in the user's repo.

## Tables

```sql
repos     (id, path UNIQUE, created_at)
chains    (id, repo_id, branch, base, status, created_at, updated_at,
           UNIQUE(repo_id, branch))
           -- status: active | merged | abandoned
changes   (id, chain_id, change_key, position, status,
           UNIQUE(chain_id, change_key))
           -- status: pending | approved | changes_requested
revisions (id, change_id, number, commit_sha, parent_sha, effective_tree,
           fixup_shas, message, created_at, UNIQUE(change_id, number))
           -- number: 1-based patchset number
           -- effective_tree: tree sha with this change's fixups folded in
           --   (equals the commit's own tree when there are no fixups;
           --    NULL when folding hit a merge conflict)
           -- fixup_shas: JSON array of folded fixup commit shas, in order
comments  (id, change_id, revision_number, file, line, side, body, state,
           review_id, created_at, updated_at)
           -- state: draft | published; side: old | new
           -- file NULL = change-level comment; line NULL = file-level
reviews   (id, change_id, revision_number, verdict, message, created_at)
           -- verdict: approve | request_changes | comment
events    (id, chain_id, kind, payload, created_at)
           -- id is the monotonic long-poll cursor
           -- kind: chain_updated | review_submitted | chain_merged
```

## Change identity (`change_key`)

A change must keep its identity while its commit sha changes (rebase, amend,
autosquash). Matching, in priority order:

1. **`Change-Id:` trailer** in the commit message (gerrit-style, any opaque
   token). Agents are told to add one — see agent-workflow.md.
2. **Exact sha** — commit unchanged since last scan.
3. **Patch-id** (`git patch-id --stable` equivalent) — same diff, new sha
   (pure rebase).
4. **Subject** — first line matches an existing change in this chain whose
   commit is no longer in the branch.

Unmatched commits become new changes; existing changes whose commit
disappears and matches nothing are deleted (their comments/reviews go with
them — the agent dropped the commit).

## Scan algorithm (runs on push and on every chain read)

1. Resolve `base` and branch tip. If the branch ref is gone → chain status
   `abandoned`. If tip is an ancestor of base, or every change's patch-id
   appears in `base@{<recent>}..base` → `merged`. Both emit `chain_merged`
   and stop. (Status can flip back to `active` if a later scan finds the
   branch alive again — e.g. force-push reusing the name.)
2. Walk `base..tip` oldest-first. Split commits into **regular** and
   **fixup** (`fixup! ` / `squash! ` subject prefix).
3. Attach each fixup to its target: searching *earlier* commits in the walk —
   exact subject match, then subject-prefix match, then sha-prefix match
   (autosquash semantics). A fixup whose target is missing is treated as a
   regular change. Fixups of fixups chain to the root target.
4. Match regular commits to existing change rows (identity rules above);
   create/delete/reposition as needed.
5. For each change compute its **effective state**:
   `(commit_sha, [fixup shas in branch order])`. Effective tree = commit's
   tree with each fixup's diff folded in via in-memory three-way tree merge
   (`merge_trees(ancestor=fixup_parent, ours=acc, theirs=fixup_tree)`;
   trees are written to the repo odb — unreachable objects, gc-able).
   Conflict → `effective_tree = NULL`; UI shows the fixup separately with a
   "needs rebase" flag.
6. If the effective state differs from the latest revision → insert revision
   `number+1`. Status effects:
   - same diff as previous revision (patch-id equal, e.g. pure rebase) →
     keep status (approvals survive rebases);
   - otherwise → status back to `pending` (reviewer must re-look).
7. Any structural difference emits one `chain_updated` event.

A change's diff is always `parent_sha → effective_tree` of the selected
revision. Earlier changes' fixups are *not* folded into later changes'
parents — each change's diff is self-contained against its real parent.
The interdiff between revisions m and n is `effective_tree(m) → effective_tree(n)`.

## Status machine

```
change:  pending ──approve──▶ approved      (review verdict)
         pending ──request_changes──▶ changes_requested
         any ──new revision with different diff──▶ pending
chain state (derived, not stored):
         any change changes_requested  → agents_turn
         else any change pending       → waiting_for_review
         else                          → ready_to_merge
chain status (stored): active → merged | abandoned   (scan step 1)
```

`comment` verdict publishes drafts without moving the change's status.
