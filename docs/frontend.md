# Frontend

React 19 + TypeScript, Vite, in `web/`. Libraries: `react-router-dom`,
`@tanstack/react-query` (fetching + caching), `highlight.js` (diff
syntax highlighting). Keep the dependency list short; justify additions in
the commit message.

`web/src/api/types.ts` mirrors [api.md](api.md) exactly — never invent
shapes in components. `web/src/api/client.ts` is the only place `fetch`
happens.

The unit of state is the **change** (a `Change-Id`, scoped to a repo); a
**chain** is derived, never stored — a path walked from a tip back to the
repo's canonical branch ([data-model.md](data-model.md), [api.md](api.md)
"Chains"). The web reflects that: a change page reads `?revision` to pick a
patchset, and `?revision` _is_ the choice of chain context. The web fetches
with react-query — on mount, on window focus, and on mutation invalidation; a
reviewer action invalidates the affected queries so its result shows at once. It
does not (yet) consume the change websocket (`WS /api/stream`), which serves the
agent-side followers (`nit wait`, `nit log --follow`); moving the UI onto those
events is the planned replacement for live refresh.

## Pages

- `/` **Repos** — table of registered repos: git-common-dir path (its
  identity _and_ name), its `base_branch`, and a live **active-chain count**
  (the repo's tip-commit count, derived). Click through to a repo's chains.
- `/repos/:id` **Dashboard** — the repo's chains as collapsible drawers, one
  per derived **tip**. The drawer header is the summary: best-effort name
  (resolved server-side at query time), state badge (`WAITING FOR REVIEW`
  amber / `AGENT'S TURN` blue / `APPROVED` green / `MERGED` gray, plus a gray
  `PARTIAL` while the agent is still pushing), a status-dot preview of the
  path in order, and the updated time. Expanding a drawer reveals the chain's
  changes in place — one row per member: position (0-based), subject, short
  sha, status chip, the pinned revision (`r{n}`), and activity counts
  (comments / drafts / unresolved). A member pinned to an older patchset than
  its latest carries a `NEWER ELSEWHERE` badge. The list is `GET
/api/chains?status=active`, whose `ChainSummary.path` already carries every
  member entry — the drawer renders from it with no further fetch.
  Merged/abandoned tips drop off (visible only with `status=all`); a
  partially-landed stack stays — any one live member keeps it on the page,
  while a member that has landed leaves the path (the walk stops at the
  canonical branch), so the drawer shows only the open members. A
  drawer opens (and scrolls into view) when deep-linked as `#chain-<tip>` —
  the review breadcrumb's chain link and the post-review "back to the chain"
  jump both target it. There is no orphaned-changes section — a change no tip
  reaches is simply off every path (reachable by id).
- `/changes/:id` **Review** (the core view):
  - **Revision & chain context**: the page reads `?revision=N` (default: the
    change's latest). The selected patchset's `parent_sha` determines the
    derived path, so `?revision` selects the chain context — there is no
    `?chain` param. The chain is fetched as `GET /api/chains/{id}?revision=N`
    (re-fetched per `selected`, so switching patchsets re-roots the
    breadcrumb onto that revision's chain). `ChangeDetail.chains` lists every
    tip walking through this change.
  - **Header**: a breadcrumb — `repo {id} / {chain name} / change {pos} of
{len} · {change_key}` — the position and status read from **this change's
    entry in the selected revision's chain** (a `PathEntry`), not from
    `ChangeDetail` (which is change-wide). Subject + status chip, then a
    commit / parent / time meta line and the published-reviews strip (each
    review carries its `revision`).
  - **Diff range**: a Gerrit-style dropdown pair in the diffbar,
    `Base|rM → rN`. The right select is the revision under review (the
    new/TO side, the `revision` URL param); the left drives `?against=` —
    `base` (full diff vs parent) or an earlier `rM` (interdiff `rM → rN`).
    Default: `Base → rN` (the full diff vs parent); the reviewer picks an
    interdiff from the dropdown. Each `rN` option
    shows its own thread count. The two selects are independent coordinates;
    switching the revision preserves a still-valid numeric base, resets an
    invalid one to Base.
  - **Commit message**: a synthetic `/COMMIT_MSG` file ([api.md](api.md))
    leads the file list, commentable like code — the full message lives
    there, not the header.
  - **Sidebar** (sticky, viewport-bounded): chain nav on top (one row per
    path member — status dot, position, subject, `NEWER ELSEWHERE` badge,
    unresolved count; current highlighted; collapsible), file list below
    (diff totals, then per file: path, status letter, +/- counts). The two
    share the height so neither pushes the other off-screen. Selecting a file
    expands and scrolls to it; scroll-spy highlights the file under the sticky
    chrome. One scroll column; unified ⇄ side-by-side toggle persisted in
    localStorage; `[`/`]` file nav; `n`/`p` change nav over the path.
  - **Files** are collapsible and start collapsed except `/COMMIT_MSG`. Each
    header shows an `N comments` tally for threads visible in the shown
    range. Collapse state resets per diff (other change, revision, or base).
  - **Diff**: monospace, old/new line-number gutters, add/del coloring,
    per-line syntax highlighting (by extension; skipped silently when
    unknown), hunk separators. A `drift: true` line (base movement folded
    into a rebase interdiff) is tinted and excluded from counts
    ([api.md](api.md) "Rebase-aware interdiffs").
  - **Comments**: select diff text (partial or multi-line, one side at a
    time) and press `c` → the editor opens under the selection with the
    range recorded ([api.md](api.md) "Range comments"); `c` on a line
    comments it. Either column is commentable — the new column anchors to the
    selected revision, the old to its parent (or, in an interdiff, the FROM
    revision's side). A comment renders only when its `(revision, side)` is
    one of the two displayed sides ([api.md](api.md) "Comment placement"); a
    thread on a shown side but outside the rendered hunks groups at the top of
    its file with its `line_text`, and a thread pinned to a revision that is
    neither FROM nor TO drops out entirely. Ranged threads tint their text;
    drafts get a dashed border + `DRAFT` tag. The server returns published
    **threads** and the reviewer's **drafts** separately; the client merges
    them (`assembleThreads`).
  - **Thread resolution** is drafted, gerrit-style ([api.md](api.md) "Thread
    resolution"): Reply / Resolve / Reopen open the editor with a `Resolved`
    checkbox; saving stores a draft reply (empty body allowed when only the
    checkbox changed). The badge shows the pending state with a `· unsaved`
    hint, applied when the review submits.
  - **Review bar** (sticky bottom): draft count, pending unresolved count,
    and `Review (a)` opening the reply modal — cover message + `Approve` /
    `Request changes` / `Comment` → `POST /api/changes/{id}/reviews` against
    the selected `revision`, then navigate to the next pending member in path
    order (else back to the chain, else home). On a 409 (the targeted
    patchset went stale, or the agent pushed meanwhile) the modal stays open,
    keeps the message and drafts, refetches, and re-offers submit.
- 404/error: plain message + link home. Loading: skeleton rows, no
  spinner-only screens.

## Design language

Expert-dense, dark-first (single dark theme for v1). Background
`#0d1117`-ish, mono for code/shas, sans for chrome. Color discipline lives in
`components/badges.tsx`: amber = needs reviewer, blue = agent working,
green = approved/ready, red = changes requested/deletions, gray =
informational. The gray-not-amber rule is deliberate — `PARTIAL`, `NEWER
ELSEWHERE` and the terminal states are inert markers, so
amber stays reserved for "needs reviewer". Compact, no marketing fluff.
Keyboard shortcuts (`[`/`]` file nav, `n`/`p` change nav, `c` comment, `a`
reply modal) are optional in v1.

## Mock mode (UI work without a backend)

`VITE_MOCK=1 npm run dev` makes `client.ts` serve canned fixtures from
`web/src/api/fixtures.ts` — a contract-true in-memory implementation of
[api.md](api.md), so the whole UI (drafts, resolve, review submission, 409s)
works without a backend. The data doubles as component-test fixtures.

Chains are **derived in the mock exactly as on the server**: a fixture stores
the change set and a tip set (a `(tip_change_id, revision)` per tip), and
`mockRequest` walks each tip revision's `parent_sha` back to the repo's base
through the `commit_sha → (change, revision)` index to build the `path` — no
stored chain list. Coverage worth knowing: repo 1 (acme-runtime) has a
3-change `waiting_for_review` chain with a 2-revision change (interdiff,
resolved/unresolved threads, a `/COMMIT_MSG` thread answered by a reword,
drafts, a rebase-drift file) plus a merged tip behind `status=all`; repo 2
(quarry) a `request_changes` partial chain and an `approved` chain; repo 3
(orbit) the **B-in-two-chains** scenario — one change `B` reached by two tips
at two patchsets (tip C walks `B` at rev0, tip E at rev1), so the
newer-elsewhere badge (`latest_revision` > `revision`) renders. Revisions are
0-based, so the fixtures exercise rev0/rev1 display directly. Keep fixtures
contract-true.

## Checking your work

`npm run check` (tsc), `npm run build`, and `npm test` (vitest; jsdom +
testing-library, against the mock fixtures) must pass in the devShell.
Visual verification is the screenshot harness — see [dev.md](dev.md).
