# Reviewer-driven abandonment and explicit multi-chains

A follow-up to `plan/change-centric-model.md`, written in response to review
on the engine cutover (chain 4, change `I66a822ca`). It revisits one decision
the cutover inherited from the original plan — **who abandons a change** — and
the model consequence the reviewer wants to make first-class: **a change can
belong to more than one chain, and the API says so.**

Not yet implemented. This is the plan the reviewer asked for ("a new commit
introducing a plan for not auto-abandoning changes") so the redesign can be
reviewed before it is built. It does **not** touch merge detection, which
stays automatic. The design below has been through one adversarial-review
pass; three of the round-one open questions are now decided in the body and
flagged under "Decisions taken".

## The problem with auto-abandonment

Today a background sweep abandons a change when its latest revision is
unreachable from any `refs/heads/*` across two consecutive windows
(`plan/change-centric-model.md` "Abandon and reopen", implemented as
`gitscan::reachable_from_branches` + the `unreachable_since` window). That
makes abandonment a **derived fact about git refs**, and it is the wrong shape
for three reasons:

1. **It punishes normal git.** A commit pushed from a detached HEAD, or a
   branch the agent deletes after rebasing, is reachable from no
   `refs/heads/*` — so it is abandoned on the next sweep, even though the work
   is live and under review. The reviewer flagged exactly this on
   `gitscan/mod.rs:210`.
2. **Abandonment is a judgment, not an observation.** "This change is dead"
   is a reviewer's call. Merge is observable (the patch landed in canonical);
   abandonment is not. Deriving it from ref topology conflates "no branch
   points here right now" with "nobody wants this."
3. **It fights the multi-chain model.** When a push reparents a change's
   successor, the old successor falls off the branch and the sweep reaps it —
   silently destroying the very second chain the change-centric model is
   supposed to expose.

The fix is to make abandonment an **explicit reviewer/agent action** and let
unreferenced-but-unabandoned changes simply persist as their own chains.

## Abandonment becomes an action

`abandoned` joins `reopened` as a lifecycle entry written **on request**, never
by the timer:

- `POST /api/changes/{id}/abandon` (optional `{ "message": "..." }`) appends
  `lifecycle{abandoned}` to that change's log. Reviewer-initiated; the agent
  may also abandon its own change.
- CLI `nit abandon <change>` over that endpoint.
- UI: an **Abandon** control on the change/review page, mirroring the existing
  reopen affordance.

`nit reopen` / `lifecycle{reopened}` keeps its behavior — it already clears
`abandoned` back to the retained verdict status, and a push to an abandoned
change is still gated on "reopen first" (`plan/change-centric-model.md` Push
step 4); its now-dead `unreachable_since.remove(...)` bookkeeping is dropped
with the field. Two properties of the new writer are worth stating, because
moving abandonment off the timer changes more than just who writes it:

- **Durable, not self-healing.** The timer's abandonment was a transient
  in-memory observation that cleared on re-reachability and reset on restart
  (best-effort by design). `lifecycle{abandoned}` is an append-only log fact,
  reversible **only** by an explicit `nit reopen` — there is no automatic
  correction. A mistaken abandon (including an agent abandoning its own
  change) persists until reopened.
- **Authorless.** nit has no authentication and a change has no owner field,
  so "the agent may abandon its own change" is convention, not enforcement —
  exactly as auto-abandon was authorless. If reviewer- vs agent-initiated
  abandon ever needs distinguishing, `LifecyclePayload` gains an optional
  initiator field; not now.

**Push vs abandon (a concurrency invariant).** Abandon is a second concurrent
writer alongside push, but the per-change `proj.write()` lock (the lock
collapse landed in C2 r2) already serializes them — the append-only log plus
the single lock reduce concurrency to one check-under-lock. Every
state-changing request follows the same shape (the reviewer's formulation,
thread 27):

1. take the change's write lock;
2. **verify the action is valid**, short-circuiting on failure (e.g. a push to
   an abandoned change 409s "reopen first") — interleavings are impossible
   under the lock, so one check suffices;
3. apply the entries to a **clone** of the projection (the only fallible step);
4. persist to the DB;
5. **swap** the clone in as the live projection, then release the lock (dropped
   naturally).

If any step fails the transaction rolls back and the live projection is
untouched, so the log can never get ahead of the projection or vice-versa.
This is what closes the push/abandon race: a push that read `Active` before a
concurrent abandon committed re-checks lifecycle in step 2 under the held lock
and 409s, rather than resurrecting the change. (`append_to_change_with` today
already does clone-validate-then-persist; folding it to install that same
validated clone — step 5's swap, one fold instead of two — is the tidy-up the
reviewer describes.)

### What the timer keeps

The lifecycle timer stays — as a **merge-only** sweep. Everything under "The
per-change merge timer" survives: `landed_revision`, the canonical
`fork..canonical` window, the Change-Id-then-patch-id match, prefix merge.
What is **removed**:

- `gitscan::reachable_from_branches` and its caller in `sweep_lifecycle`.
- `AppState::unreachable_since` and the two-sweep window
  (`NIT_ABANDON_SECS`, the second half of the sweep).
- The tip-commit set's role in deriving state. Tips remain a read-time
  derivation (leaves over latest revisions) used to _enumerate_ chains; they
  never gate liveness, because nothing is reaped.

A change is **live** until it is merged or explicitly abandoned. "Off every
branch" is no longer a terminal condition — it is just a change whose chain
has no git ref naming its tip (the chain is still enumerable and viewable).

One cost this shifts, flagged under "Still open": auto-abandon silently
**bounded the merge sweep's working set** (an off-every-branch change was
reaped and dropped from the non-terminal set). With nothing reaped, the set is
every never-merged, never-abandoned change, each re-walked every tick. The
parent plan already flagged sweep cost as open for active tips
(`plan/change-centric-model.md` "Sweep cost"); this widens it to all live
changes.

## A change belongs to multiple chains

The reviewer's worked example (verbatim intent):

```
logical chain today:     canonical → A → B → C
push of C reparented:    canonical → A → C        (C's parent_sha is now A)
```

`B` is no longer on `C`'s path, but it is not dead. `B` and `C` both descend
from `A`, so `A` now sits on **two** chains:

```
chain(tip = B):  base → A → B
chain(tip = C):  base → A → C
```

This already falls out of the cutover's derivation — `tips()` returns both
leaves and `chains_through(A)` walks both (the `b_in_two_chains` case is built
and tested in `chain.rs`). The reviewer's **option 2** ("a chain endpoint for
a change returns multiple chains") is therefore mostly already in the model;
the work is to surface it correctly and to make abandonment cohere with it.

### The multiplicity already lives on the change resource

`ChangeDetail.chains` is a `Vec<ChainRef>` — "every tip walking through this
change, each with the patchset it pins" (`crates/nit/src/api/types.rs`). That
**is** option 2: ask for a change, get back every chain it participates in. So
**no endpoint reshape is needed**, and in particular `GET /api/chains/{id}`
stays a single flat `Chain` addressed by a **tip** change (with the existing
`tip_for` fallback to the change's own commit). Keeping it flat is deliberate:
it is the load-bearing endpoint of the agent push/wait loop (`nit status` /
`wait` / `log`, `resolve_change` all read it), and the CLI always hands it the
tip it just pushed, so there is nothing to disambiguate there. (An earlier
draft of this plan proposed reshaping that endpoint to return a list; that was
both breaking and redundant with `ChangeDetail.chains` — dropped.)

### Abandonment is derivation-inert

Per the reviewer (thread 28), abandonment is a **per-change status and nothing
more** — it does not affect chain state or membership. This is simpler than
flagging chains and removes a whole state:

- `ChainState::HasAbandoned` is **removed**. A chain's rolled-up `state`
  (`derive_state`, `chain.rs`) is derived from review verdicts and merge only
  (`agents_turn` / `waiting_for_review` / `approved` / `merged`); an abandoned
  member never rolls up to a chain-level flag. The member's
  `ChangeStatus::Abandoned` shows inline on its path entry, and the agent
  decides what to do with it. (`has_abandoned` was also a misnomer — a
  set-contains, not a state; deleting it sidesteps the naming.)
- An abandoned change stays a **member and a tip**: `tips()` / `chains_through`
  stop filtering abandoned (they still drop **merged**, which has landed). So
  an abandoned leaf still enumerates as its own chain — no `all_tips`
  special-case, and the empty-`chains` regression simply cannot arise.

The reviewer's rationale: abandonment is the agent's signal, not the chain's
verdict. In an `A → B` chain where the reviewer abandons `A` but not `B` (and
`B` builds on `A`'s commit), nit shouldn't editorialize the chain's state — the
agent reads "`A` abandoned" and decides whether it can drop `A` (rebase `B` off
it) or must pause for more feedback. Two consequences settled in implementation,
not in the model: the **merge sweep** still skips abandoned changes (no point
merge-checking a dead change — an optimization), and the **dashboard** may
de-emphasize a chain whose tip is abandoned (a display choice).

Test: abandon a live tip, then its `ChangeDetail.chains` (and
`GET /api/chains/{it}`) still resolves to one chain; the chain's `state` is
unaffected by the abandonment.

### CLI: ambiguity reuses the existing flag

`nit status` / `nit log` already accept `--chain <tip change id>` to address a
specific chain (`crates/nit/src/cli.rs`). The agent's normal path passes the
tip it pushed, so it is unambiguous. The only new behavior: when a change is
addressed that is on more than one chain and no `--chain` is given, the command
**errors** with the candidate tips rather than guessing. No new flag — `--chain`
is the escape hatch, and only the change-id-addressed path can hit the error
(the default HEAD-tip resolution already yields a single tip).

### UI: stack the chains

The change/review page reads `ChangeDetail.chains` and renders each chain it
lists, stacked, so a shared ancestor visibly belongs to both. `ChainNav`
already draws one path; it gains an outer loop over the refs (fetching each
chain's path by its tip).

## Keep-ref GC follows abandonment

The cutover deferred keep-ref deletion (the reviewer's thread on
`gitscan/objects.rs:52`); there is no deletion code today (`ensure_keep_ref`
only writes). With abandonment now an action, GC gains a trigger — but the
terminal action only makes a ref **eligible**; droppability is **recomputed
against the live `parent_sha` set on every edge change** (each push and each
merge-sweep tick), never decided once at the terminal moment. A revision's keep
ref is dropped only when its change is terminal (abandoned/merged) **and** no
live revision records its commit as a `parent_sha`. Recomputing against live
edges — not freezing the answer at the terminal action — is what stops a later
reparent-away from leaking the ref forever. Still safe-by-default: when in
doubt, keep (over-pinning never drops an object a walk needs).

One case to decide explicitly: an abandoned **tip** is a commit no revision
records as a parent, so by the rule above its keep ref is immediately
droppable — and after a `git gc` its diff no longer resolves, so a direct
`GET …/diff` on the abandoned change fails, undermining "keep showing it".
Decision: **retain a terminal change's own latest-revision keep ref** until an
explicit prune (or a TTL), so its diff stays viewable; only the _superseded_
revisions of a terminal change with no live child are GC'd.

## Test migration

Removing `reachable_from_branches` + the `unreachable_since` window deletes the
only path that turned "branch deleted" into "abandoned", which six tests rely
on (each does `delete_branch` then waits for the abandoned state):

- `scan_lifecycle.rs::unreachable_revision_becomes_abandoned_after_window` —
  **delete** (its premise is gone).
- `scan_lifecycle.rs::reopen_clears_abandoned_to_retained_status`,
  `::push_to_abandoned_change_409s_until_reopened`,
  `::re_push_of_unchanged_abandoned_revision_is_not_blocked` — reach the
  abandoned precondition via `POST /api/changes/{id}/abandon` instead of
  `delete_branch`; the reopen/409 behavior they assert is unchanged.
- `api_repos.rs::abandoned_chain_drops_out_of_active_chains` — abandon via the
  action; since abandonment is now derivation-inert, this becomes a **display**
  assertion (the dashboard's active view de-emphasizes/hides an abandoned tip)
  while the change still resolves to its own chain — or is dropped if the active
  view keeps listing abandoned tips. Re-decide what "active" means here.
- `cli_e2e.rs::reopen_an_abandoned_change` — abandon via `nit abandon`.

Also drop `NIT_ABANDON_SECS` from the test harness, update the
`scan_lifecycle.rs` module doc ("merged/abandoned written only by the sweep" →
"merged by the sweep; abandoned by the action"), update `status_at` for the
unchanged-but-reconfirmed chain shape, and **add a positive test** that a
branch-less but unabandoned change stays live (the new core invariant). Per
golden rule 4, `docs/api.md` (the abandon endpoint) and `docs/data-model.md`
(the status machine: abandoned is now action-written, and the `has_abandoned`
chain state is removed — abandonment no longer rolls up to a chain) change
**first**.

## What is removed

- Auto-abandonment: `reachable_from_branches`, `unreachable_since`, the
  two-sweep abandon window, `NIT_ABANDON_SECS`.
- Liveness-from-refs: a change is live until merged or abandoned, full stop.
- `ChainState::HasAbandoned`: abandonment no longer rolls up to a chain state.
- Abandoned-filtering in `tips()` / `chains_through`: they drop only **merged**
  now, so abandoned changes stay enumerable members.
- The branch-deletion setup of the six abandon tests (see Test migration).

## What is added

- `lifecycle{abandoned}` as a request-written entry; `POST .../abandon`;
  `nit abandon`; the UI Abandon control.
- The clone-validate-persist-**swap** append shape with a lifecycle re-check in
  step 2 (push-vs-abandon serialization under the one write lock).
- The multi-chain ambiguity error on `nit status` / `nit log`, **reusing the
  existing `--chain` flag** (no new flag).
- `ChainNav` stacking from `ChangeDetail.chains`; keep-ref GC recomputed on
  every `parent_sha`-edge change, retaining a terminal change's own latest ref.

## Decisions taken (were open in round one)

Settled in the body above; listed so the reviewer can override:

- **Abandon scope** (round-one open 2): abandon targets exactly one change, no
  cascade. To kill a stack, abandon each. An "abandon reachable-only-through
  this tip" convenience would be an explicit client-side multi-call, **not** a
  return of ref-reachability reaping — offered only if wanted.
- **CLI ambiguity** (round-one open 3): error on ambiguity, reuse `--chain`.
  Silently picking a chain is the bug option 1 was meant to avoid.
- **Abandoned interior change** (round-one open 4, refined by reviewer thread
  28): abandonment is **derivation-inert** — `ChainState::HasAbandoned` is
  removed, an abandoned change stays a member/tip, and the agent reasons about
  the per-change status (see "Abandonment is derivation-inert"). Simpler than
  the tip-keyed rollup r3 proposed.

## Still open for the reviewer

1. **Merge stays automatic.** Only abandon becomes manual; merge is an
   observable fact about canonical, nothing argues for making it manual too.
   Confirm.
2. **Unbounded merge-sweep set.** Without ref-reaping, the sweep re-walks every
   live change each tick. Acceptable at nit's pre-v1 scale, or add a cheap
   guard — e.g. skip the `fork..canonical` revwalk for a change no live tip
   walks through (it cannot newly land via a tip the reviewer sees)?
3. **Agent-initiated abandon guardrail.** Given no actor identity and no
   auto-correction, should an agent abandoning its own change need a
   confirmation/guard, or is explicit `nit reopen` enough?
