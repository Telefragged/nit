# Frontend

React 19 + TypeScript, Vite, in `web/`. Libraries: `react-router-dom`
(routing), `@tanstack/react-query` (fetching/polling/cache),
`highlight.js` (per-line syntax highlighting in diffs). Keep the dependency
list short; justify additions in the commit message.

`web/src/api/types.ts` mirrors [api.md](api.md) exactly — never invent
shapes in components. `web/src/api/client.ts` is the only place `fetch`
happens.

## Pages

- `/` **Dashboard** — table of active chains: branch, repo basename, state
  badge (`WAITING FOR REVIEW` amber / `AGENT'S TURN` blue / `READY TO MERGE`
  green), per-change status dots in chain order (click-through), updated
  time. Chains gone (merged/abandoned) disappear. Poll via react-query
  `refetchInterval: 5000`.
- `/chains/:id` **Chain** — ordered commit list: position, subject, status
  chip, revision count, comment/draft/unresolved counts, an "updated since
  your review (1→2)" badge when `last_reviewed_revision < revision`.
  Orphaned changes render collapsed at the bottom (comments preserved).
  `last_scan_error` / `scan_warnings` show as a banner. Click → change view.
- `/changes/:id` **Review** (the core) —
  - header: subject, expandable full message, chain breadcrumb, revision
    selector (`1 … N`, default latest), fixup messages of the shown
    revision, base info, `needs_rebase` warning banner when set;
  - interdiff: when `last_reviewed_revision` exists and is behind, default
    to the interdiff `last_reviewed → latest` with a one-click "full diff"
    escape; otherwise full diff with a "vs revision m" toggle;
  - file list (left rail): path, status letter, +/- counts; selecting
    scrolls to the file section; all files render in one scroll column
    (diffshub style), unified ⇄ side-by-side toggle persisted in
    localStorage;
  - diff: monospace, full-width gutters with old/new line numbers, add/del
    coloring, per-line syntax highlighting (language from extension; skip
    silently when unknown), hunk separators showing skipped ranges;
  - comments: click a gutter/line → inline draft editor under that line
    (file+line+side from context; in interdiff view only new-side lines are
    commentable). Published comments render as threads (replies via
    `parent_id`, author chrome for reviewer/agent, resolve toggle) under
    their `rendered_line`; comments with `outdated: true` group at the top
    of their file with their `line_text` excerpt; drafts get a dashed
    border + `DRAFT` tag and edit/delete;
  - review bar (sticky bottom): draft count, unresolved count,
    cover-message input, buttons `Approve` / `Request changes` / `Comment`
    → POST review, then navigate to the next pending change in the chain
    (or back to the chain). On a 409 (agent pushed meanwhile): keep the
    cover message and drafts, refetch, show "new revision landed", re-offer
    submit.
- 404/error states: plain message + link home. Loading: skeleton rows, no
  spinner-only screens.

## Design language

Expert-dense, dark-first (single dark theme for v1). Background `#0d1117`-ish,
mono font for code/shas (`ui-monospace` stack), sans for chrome. Amber =
needs reviewer, blue = agent working, green = approved/ready, red =
changes requested/deletions. Compact paddings; no marketing fluff; every
piece of chrome must earn its pixels. Keyboard shortcuts (`[`/`]` file nav,
`n`/`p` change nav) are welcome but optional in v1.

## Mock mode (UI work without a backend)

`VITE_MOCK=1 npm run dev` makes `client.ts` serve canned fixtures from
`web/src/api/fixtures.ts` (a realistic chain: 3 changes, one with 2
revisions + fixup, drafts, published thread, binary + renamed files in the
diff). Keep fixtures contract-true; they double as component-test data.

## Checking your work

`npm run check` (tsc) and `npm run build` must pass inside the devShell.
Visual verification happens through the screenshot harness — see
[dev.md](dev.md).
