## Reviewer decisions (drafts)

Like comment threads, a reviewer's **decision** on a change is **drafted, not
immediate** (docs/data-model.md "Reviewer decisions"): the review modal stages a
`Decision` and a cover message in the `draft_reviews` side table (one mutable
row per change, reviewer-private, never in the log), and the chain-level batch
submit (`POST /api/chains/{id}/submit`, "Chains") publishes every member's
staged decision at once. A `Decision` is a verdict (`approve`,
`request_changes`, `comment`) **or** a lifecycle action (`abandon`, `reopen`),
so the modal offers all of them in one place — abandonment is a decision, not a
separate button.

- `PUT /api/changes/{id}/decision` —
  `req: {"decision": "approve" | "request_changes" | "comment" | "abandon" | "reopen", "message": "…"}`
  → StagedDecision. Stages (or overwrites) the change's draft decision. `message`
  is optional (the cover note for a verdict, the reason for `abandon`). Validated
  only as an enum value — legality against the change's lifecycle is checked at
  submit time, since a draft is reviewer scratch. 404 if the change is unknown.
- `DELETE /api/changes/{id}/decision` → 204 — discard the staged decision. 404
  if the change is unknown (a no-op when nothing is staged is still 204).

On batch submit each staged decision publishes **at the revision its chain path
pins on the member** (the path is the authority — a change-wide decision row
stores no revision, so the B-in-two-chains member publishes at rev0 from one
chain and rev1 from the other): a verdict drains the change's comment drafts
(their staged `resolved` decisions included) into one `review` log entry, sets
the `(change, revision)` status to the verdict, and applies each thread's
resolution in draft order; `abandon`/`reopen` append a `lifecycle` entry, and
any comment drafts on the change still drain into a `comment` review in the
**same** per-change transaction so they are never stranded. A member whose
staged decision is illegal for its current lifecycle (a verdict on a
merged/abandoned change, a `reopen` on a live one) is skipped into
`BatchSubmitResult.errors` and keeps its row. Batch submit is the **only** way
a reviewer verdict reaches the log — there is no immediate single-change submit.
