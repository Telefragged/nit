# Frontend

React 19 + TypeScript, Vite, in `web/`. Libraries: `react-router-dom`,
`@tanstack/react-query` (fetching/polling/cache), `highlight.js` (diff
syntax highlighting). Keep the dependency list short; justify additions in
the commit message.

`web/src/api/types.ts` mirrors [api.md](api.md) exactly — never invent
shapes in components. `web/src/api/client.ts` is the only place `fetch`
happens.

## Pages

- `/` **Repos** — table of registered repos: git-common-dir path (its
  identity _and_ name) and active-chain count. Click through to a repo's
  chains. Polls (react-query `refetchInterval: 5000`).
- `/repos/:id` **Dashboard** — the repo's chain list: branch, base, state
  badge (`WAITING FOR REVIEW` amber / `AGENT'S TURN` blue / `APPROVED`
  green, plus a gray `PARTIAL` while the agent is still pushing), per-change
  status dots in order, updated time. Merged/abandoned chains disappear.
  Polls.
- `/chains/:id` **Chain** — header (branch, state badge, `PARTIAL`) and the
  ordered commit list: position, subject, status chip, revision / comment /
  draft / unresolved counts, an "updated since your review (1→2)" badge when
  `last_reviewed_revision < revision`. Orphaned changes render collapsed at
  the bottom (comments preserved); `last_scan_error` shows as a banner.
- `/changes/:id` **Review** (the core view):
  - **Header**: subject, chain breadcrumb, base info.
  - **Diff range**: a Gerrit-style dropdown pair in the diffbar,
    `Base|rM → rN`. The right select is the revision under review (the
    new/TO side, tracked in the `revision` URL param); the left drives
    `?against=` — `base` (full diff vs parent) or an earlier `rM`
    (interdiff `rM → rN`). Default: the interdiff `last_reviewed → latest`
    when `last_reviewed_revision` is behind, else Base → latest. Each `rN`
    option shows its own thread count.
  - **Commit message**: a synthetic `/COMMIT_MSG` file (docs/api.md) leads
    the file list, commentable like code — the full message lives there,
    not the header.
  - **Sidebar** (sticky, viewport-bounded): chain nav on top (one row per
    change — status dot, position, subject, unresolved count; current
    highlighted; collapsible), file list below (diff totals, then per file:
    path, status letter, +/- counts). The two share the height so neither
    pushes the other off-screen. Selecting a file expands and scrolls to it;
    scroll-spy highlights the file under the sticky chrome. One scroll
    column; unified ⇄ side-by-side toggle persisted in localStorage; `[`/`]`
    file nav.
  - **Files** are collapsible and start collapsed except `/COMMIT_MSG`. Each
    header shows an `N comments` tally for threads visible in the shown
    range. Collapse state resets per diff.
  - **Diff**: monospace, old/new line-number gutters, add/del coloring,
    per-line syntax highlighting (by extension; skipped silently when
    unknown), hunk separators.
  - **Comments**: select diff text (partial or multi-line, one side at a
    time) and press `c` → the editor opens under the selection with the
    range recorded (docs/api.md "Range comments"); `c` on a line comments
    it. Either column is commentable — the new column anchors to the
    selected revision, the old to its parent (or, in an interdiff, the FROM
    revision's side). A comment renders only when its `(revision, side)` is
    one of the two displayed sides (docs/api.md "Comment placement"); a
    thread on a shown side but outside the rendered hunks groups at the top
    of its file with its `line_text`. Ranged threads tint their text; drafts
    get a dashed border + `DRAFT` tag. The server returns published
    **threads** and the reviewer's **drafts** separately; the client merges
    them (`assembleThreads`).
  - **Thread resolution** is drafted, gerrit-style (docs/api.md "Thread
    resolution"): Reply / Resolve / Reopen open the editor with a `Resolved`
    checkbox; saving stores a draft reply (empty body allowed when only the
    checkbox changed). The badge shows the pending state with a `· unsaved`
    hint, applied when the review submits.
  - **Review bar** (sticky bottom): draft count, pending unresolved count,
    and `Review (a)` opening the reply modal — cover message + `Approve` /
    `Request changes` / `Comment` → POST review, then navigate to the next
    pending change. On a 409 (agent pushed meanwhile) the modal stays open,
    keeps the message and drafts, refetches, and re-offers submit.
- 404/error: plain message + link home. Loading: skeleton rows, no
  spinner-only screens.

## Design language

Expert-dense, dark-first (single dark theme for v1). Background
`#0d1117`-ish, mono for code/shas, sans for chrome. Amber = needs reviewer,
blue = agent working, green = approved/ready, red = changes
requested/deletions, gray = informational (the `PARTIAL` badge — amber
stays reserved for "needs reviewer"). Compact, no marketing fluff. Keyboard
shortcuts (`[`/`]` file nav, `n`/`p` change nav, `c` comment, `a` reply
modal) are optional in v1.

## Mock mode (UI work without a backend)

`VITE_MOCK=1 npm run dev` makes `client.ts` serve canned fixtures from
`web/src/api/fixtures.ts` (a realistic chain: 3 changes, one with 2
revisions, drafts, a published thread, binary + renamed files). Keep
fixtures contract-true; they double as component-test data.

## Checking your work

`npm run check` (tsc), `npm run build`, and `npm test` (vitest; jsdom +
testing-library, against the mock fixtures) must pass in the devShell.
Visual verification is the screenshot harness — see [dev.md](dev.md).
