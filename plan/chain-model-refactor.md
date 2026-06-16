# Plan: repo-anchored chains, push-by-ref, timed merge detection

High-level design for reworking nit's chain/change model. This document is
the unit under review; it stays at the model level and leaves code/schema
specifics to the implementation stage. Database schemas and wire contracts
are treated as throwaway — this is a redesign as if it were the first
decision made (CLAUDE.md golden rule 8).

It has been through two adversarial review passes and several rounds of
reviewer feedback. Design decisions are **[decision]** notes (also raised as
review threads); residual risks are in "Open risks".

## Why change

1. **Chains carry a `(path, branch, base)` that callers must re-supply.**
   `nit push`/`nit ready` require `--repo` and `--branch`; reads optionally
   derive them from the cwd. The split is easy to get wrong — an agent that
   omits or mis-passes a flag pushes to the wrong place or forks a duplicate
   chain. The identity an agent manages by hand is exactly the part that
   should be inferred from where it already is.

2. **`base` has a silent default (`main`).** A defaulted base is a guess
   baked into every push; it belongs to the repository, stated once.

3. **The server re-walks a chain against its moving base on read.** A chain
   is re-scanned on (throttled) dashboard/chain reads as `base..tip`, so a
   rebased `main` injects commits into a chain's review with no push behind
   them — review state that "just shows up." This re-walking is what must
   go. _Merge detection_ — noticing a chain's changes have landed in base —
   is the opposite operation (it only ever closes a chain, never adds
   reviewable commits); it stays, moved off read-time onto a timer (below).

The append-only per-chain event log and its `/events` broadcast are kept.
What goes is the branch/path/base the chain drags around and the read-time
re-walking of chains.

## The new model in one paragraph

A **repository** is registered once and names its **base branch**. A
**chain** is a persistent review identity with no branch, path, or base of
its own — it _is_ the set of changes (by `Change-Id`) pushed under it, plus
that history's log. **A `Change-Id` belongs to exactly one chain, for life**
(chains are append-only; a change is never moved between them). `nit push
<ref>` is the only way review **content** changes: the server resolves the
repo from the cwd's git-common-dir, walks `merge-base(base, ref)..ref` into
an ordered list of changes, routes them to their owning chain by `Change-Id`,
and appends one `revisions` entry. Reads never re-walk a chain. A background
**timer** periodically checks each open chain against the base: when its
changes have all landed in base it **auto-closes** the chain (the existing
merge quorum, on a timer instead of on reads). The timer never re-walks a
chain's tip, so a moving base can only retire a chain, never inject content.

## Repositories carry the base

- A repo is `(id, git_dir, base_branch)`. `git_dir` stays the canonical
  git-common-dir (identity + display name, shared across worktrees);
  `base_branch` is the base (`main` here), resolved freshly when needed.
- A new **`nit repo create --base <branch>`** registers the repo for the
  cwd's git-common-dir with an explicit, required base. There is **no
  default base** and **no lazy auto-creation** on first push: pushing into
  an unregistered repo errors and tells the agent to run `nit repo create`
  once. **[decision: base lives on the repo, set once, no default — a
  deliberate departure from the "minimal repo registry" rule; base is
  genuinely repo-level and not safely defaultable. Implementation also
  strips the old per-push `--base` and its `main` default from docs/skill.]**
- `nit repo list` / `nit repo move` carry over; `move` is unchanged.
- **`nit repo set-base <branch>`** changes the base later but **refuses
  while the repo has open chains** (their fork points and merge detection
  would shift); the check-and-apply runs under the repo lock (Concurrency).

## `nit push <ref>` — derive everything

`nit push [<ref>] [--partial]` takes an optional git ref (default `HEAD`)
and nothing else. The repo is found from the cwd's git-common-dir; the base
comes from the repo. There is no `--repo`, `--branch`, or `--base`. One push:

1. Resolves the repo (error if the cwd's git-common-dir is unregistered).
2. Resolves `ref` to a tip commit and computes `merge-base(base, tip)`.
3. Walks `fork..tip` oldest-first into changes, validating that every commit
   carries its own **unique `Change-Id:`** and that there are **no merge or
   root commits** (the diff/identity model needs a single first parent).
   **`fixup!`/`squash!` commits are no longer special-cased** — in the
   Change-Id-required model they are ordinary commits judged solely by their
   `Change-Id` (an un-squashed fixup that lacks its own trailer fails the
   Change-Id rule, not a separate one). **An empty walk** (`tip` is base or
   already in it) **is rejected** ("no commits between base and ref —
   nothing to review"); closing a landed chain is the timer's job.
4. **Routes to a chain** by `Change-Id` ownership (next section), recording
   the pushed ref's human name for the chain's title (Chain identity).
5. Reconciles the target chain's live set against the walk and appends one
   `revisions` entry iff the structure changed — the same fold as today, now
   push-triggered. A live change absent from the walk **orphans** (today's
   lossless behavior); the timer, not the push, decides if a chain merged.

`nit ready` becomes `nit ready [<ref>]`: the same push clearing the sticky
`partial` flag. The everyday call is a bare `nit push` / `nit ready` from
the worktree you are building in.

## Chain routing — which chain a push belongs to

Because a `Change-Id` belongs to exactly one chain, the target is **fully
determined** by the walk's `Change-Id`s — no tie-break, no handle to pass:

> Collect the owning chains of the walk's existing `Change-Id`s.
>
> - **none** (all `Change-Id`s new) → a **new chain**;
> - **exactly one** owning chain → **that chain**; new `Change-Id`s join it,
>   and any of its changes orphaned (a transiently-dropped stack) **reattach**;
> - **two or more** owning chains → **rejected**.

A push reaching two chains means the agent built a branch mixing commits that
belong to different chains (e.g. rebased one stack onto another). nit will not
move a change between chains, so the push is rejected, naming the offenders:

```
Error: this push mixes changes from chain 1 (Iaaa…) and chain 2 (Ibbb…).
A change belongs to one chain — give the out-of-place commit a new Change-Id
(it becomes a new change here), or push the stacks separately.
```

**[decision: routing is Change-Id-determined; there is **no `--chain` on
push** and no silent tie-break. An earlier draft hinted `--chain` for an
ambiguous push, but once a `Change-Id` owns exactly one chain (the
no-migration rule below) `--chain` could only "resolve" a mix by reassigning
ownership — which is the migration we are rejecting. So the ambiguous push is
fixed by editing the `Change-Id`, not by a flag. `--chain` survives only on
the read/lifecycle commands, where `HEAD` may not map to a chain. **Raised
for confirmation** — this reverses the earlier "point a push at `--chain`"
note now that migration is gone.]**

### What "a commit moved between two chains" does — nothing special

A change belongs to one chain for life; there is **no migration**. "Moving a
commit to another chain" is simply **giving it a new `Change-Id`**: the new
id is a new change that joins wherever it is pushed, while the old change
stays **orphaned in its original chain** with all its history (lossless, but
its threads/comments do **not** follow — they belong to the old change). This
needs no server mechanism at all — the agent edits the trailer, and routing
does the rest. **[decision: reject migration outright (the reviewer's call).
Chains are append-only and a change belongs to a chain; copying review
history across chains added real complexity (which source to pull from, id
remapping, verdict-or-pending, a duplicated orphaned copy) for a rare move.
"Move" = re-id, comments not carried. Rejected alternatives: M1
(server adopts the change into the target and orphans it in the source — a
silent cross-chain move), M2 (migrate history — the complexity just removed),
M4 (durable co-membership — breaks change = unit of review).]**

### What stays inexpressible

A push is the **current full state of one chain** (`fork..tip`). You can't
peel one change into a new chain by pushing a sub-range, and you can't fuse
two existing chains into one (their `Change-Id`s can't be reassigned) —
combining means re-id'ing one stack (losing that side's history) or keeping
them separate. Independent work means fresh `Change-Id`s. Two features
sharing a `Change-Id`-bearing prefix collapse into one chain — see Open
risks.

## Lifecycle — timed merge detection, explicit abandon/reopen

A chain's review **content** changes only on push; reads never re-walk a
chain. Closure works two ways:

**Merged — a periodic merge-detection timer.** A background task
periodically runs, **for each open chain**, the **existing chain-level merge
quorum**: does the chain's live set appear in `fork..base` (`Change-Id`
first, then patch-id, with its all-or-nothing + at-least-one-real-match
guards — `gitscan::merged_quorum`, unchanged)? If the whole chain has landed
→ append `chain_closed{merged}` and reap its keep-refs. This is **exactly
today's merge test, moved from read-triggered to timer-triggered.** It
**never re-walks a chain's tip, never recomputes `fork..tip`, never appends
`revisions`** — it can only close a chain, so a moving base cannot inject
review content (problem 3). The quorum anchors on each chain's **own recorded
fork** (the base-most live change's parent), a true ancestor immune to base
force-push/rebase, and is **idempotent and self-correcting** — a tick that
fails or is skipped retries next tick (no cursor to corrupt; an in-memory
"base unchanged" check may skip idle repos as a pure optimization). Each
close goes through the chain's existing gate + `commit_entries`, so the
append/fold/publish discipline is unchanged.

**[decision: merge detection stays CHAIN-LEVEL (close when the whole chain
has landed), not per-change — approved. A background TIMER (poll), not an
explicit `nit close` (which would put closure on an action that can be
skipped) and not a base-ref filesystem watch (merge detection is
latency-tolerant — it only retires, never injects — so a poll is the floor
and an event watch is a later optimization).]**

**Abandoned / reopened — explicit verbs** (resolved from the cwd's `HEAD`
chain or `--chain <id>`):

- **`nit abandon [--chain <id>]`** — declare a chain dead (a discarded
  worktree leaves no git signal to auto-detect). Marks it abandoned, reaps
  keep-refs. The branch-missing timer and abandonment-by-disappearance are
  deleted (chains have no branch to watch).
- **`nit reopen [--chain <id>]`** — flip a closed chain back to active (a
  mistaken auto-close, or resuming abandoned work). Keep-refs re-established
  on the next push.

## Resolving "my chain" for read commands

`nit wait` / `nit status` / `nit comment` / `nit log` / `nit abandon` /
`nit reopen` have no branch to key on. Default resolution: **walk the cwd's
`HEAD` into `Change-Id`s and find the chain that owns them**, preferring an
**open** chain. Then:

- exactly one open owner → use it;
- none open, but a **closed** chain owns them → use it (so a read right after
  the timer auto-closes a chain still resolves and delivers the
  `chain_closed` entry, instead of erroring);
- **more than one** open owner, or no owner at all → **error**, naming what
  was found and pointing at `--chain <id>`.

`--chain <id>` is the **only** place a chain handle is taken — for monitors,
other checkouts, and detached/post-merge states where `HEAD` doesn't map. It
is kept out of the agent-facing docs/skill (discoverable via the error and
the command help, "only when you are certain which chain you mean").

## Concurrency

- A **per-repo async mutex** guards _push routing + optional chain creation +
  first-append chain-id assignment_, released before the per-chain gate is
  taken for the reconciling append. Two concurrent first-pushes of one stack
  can't both decide "no owner"; pushes to two _different existing_ chains
  stay parallel. `nit repo set-base` takes this lock for its whole
  check-and-apply, so its open-chain count can't race a concurrent
  first-push.
- The **merge-detection timer needs no new lock model**: it runs the merged
  test per chain under that **chain's existing gate** + `commit_entries`
  (exactly as today's read-scan did), serializing against reviews, comments,
  and pushes to the same chain the way they already serialize.
- The read-scan throttle is removed with the read-scans it bounded.

## Chain identity, display, and storage

- The `chains` row shrinks to **`(id, repo_id)`** — nothing else. Branch,
  base, and path are gone; membership is folded from the log;
  `created_at`/`updated_at` are derived from the log's first/last entries
  (the UI shows `updated_at`), not stored.
- A chain has no branch as identity, but a push **records the human name of
  the ref it pushed** — the branch or tag the ref resolves to, falling back
  to `detached HEAD at <short-sha>` — in its log entry. The chain's **display
  title is the latest push's ref name** (derived from the log, not a stored
  column), with the numeric chain id as the stable handle.
- The `Projection` holds `repo_id` (+ `chain_id`); **`git_dir` and base are
  joined from the repo registry at use, not cached** — so a `repo move` /
  `set-base` needs no projection refresh. The change wire enum is unchanged
  (`pending | approved | changes_requested | commented | orphaned`); "merged"
  stays chain-level (`chain_closed`).

## What is removed

- `--repo`, `--branch`, `--base` on `push`/`ready`; the cwd+branch lookup;
  `--chain` on push.
- Lazy repo creation and the defaulted base.
- Read-time re-walking of chains (`list_repos`, `list_chains`, `get_chain`
  stop scanning) and the throttle that bounded it.
- The `fixup!`/`squash!` special-case rejection.
- The branch-missing timer and abandonment-by-disappearance; implicit
  reopen-by-push; any cross-chain change movement/migration.
- `branch`/`base`/`path`/`created_at` columns on the chain.

## What is kept

- The append-only per-chain log, the fold, `/events`, `nit wait`'s cursor —
  untouched.
- The chain-level merge **quorum** (now run by the timer, never on read),
  orphan/reattach semantics, keep-refs (maintained on push, reaped on
  close/abandon), drafts, reviews, comments, diffs, rebase-aware interdiffs.
- The `Change-Id` trailer as the change identity and the per-commit
  unit-of-review.

## Open risks and pitfalls

- **Merge close is eventual, not synchronous.** The approve action's
  fast-forward and out-of-band merges are detected within one poll interval;
  the agent's running `nit wait` is the channel that delivers `chain_closed`.
  A confirming `nit push` after the merge is rejected (empty walk), not the
  signal.
- **Combining or splitting chains is not supported.** A change can't move
  between chains, so fusing two stacks into one means re-id'ing one (losing
  that side's review history) or keeping them separate; peeling one change
  into its own chain likewise means a fresh `Change-Id`. This is the cost of
  the append-only "a change belongs to one chain" rule.
- **Shared-prefix stacks collapse into one chain.** Two features off a common
  `Change-Id`-bearing change resolve to one chain; the escape is fresh
  `Change-Id`s.
- **Prefix-merge is not auto-closed.** A cherry-pick of a strict sub-prefix
  of an open stack into base (out-of-band) leaves the chain open until the
  rest lands or it is abandoned (chain-level detection is all-or-nothing).
- **Patch-id false positives.** A change whose diff coincidentally matches an
  unrelated base commit could be counted landed; the quorum's all-or-nothing
  - at-least-one-real-match guards damp this (and `Change-Id` matches first),
    but the risk is non-zero for cherry-picks outside the nit flow.
- **Abandonment is fully manual.** A discarded worktree never `nit abandon`ed
  stays active and keeps its keep-refs pinned (a future `nit gc` is the
  reaper).
- **`HEAD`-derived resolution has blind spots** (detached, mid-rebase, a
  checkout without the stack); `--chain` is the escape hatch.

## Staging

1. **This plan**, reviewed and approved through nit.
2. **Implementation — one atomic cutover.** repo `base_branch` +
   `nit repo create` / `set-base`; push-by-ref + Change-Id routing (reject a
   cross-chain mix) + the per-repo lock; the merge-detection timer (the
   existing chain-level quorum on a poll); removal of read re-walks / the
   branch-missing path / the fixup-squash rejection; explicit `nit abandon` /
   `nit reopen`; HEAD-derived chain resolution with the closed-chain
   fallback; the shrunken `(id, repo_id)` chain row and ref-name-derived
   title. The **CLI binary, the `nit-review` skill, docs/api.md, and the web
   change in the same landing** — they hard-require the removed flags and the
   branch column today, so a partial cutover strands the dogfood loop on its
   own broken skill. After the (reset, pre-v1) DB is in place, run
   `nit repo create --base main` once in the primary checkout before the
   first post-cutover push. Schema and wire contracts are rewritten as
   needed; no migration is owed.
