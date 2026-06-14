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
  green) plus a gray `PARTIAL` badge while the agent is still pushing
  (`chain.partial`), per-change status dots in chain order (click-through),
  updated time. Chains gone (merged/abandoned) disappear. Poll via
  react-query `refetchInterval: 5000`.
- `/chains/:id` **Chain** — header: branch, state badge, the gray `PARTIAL`
  badge while `partial`. Ordered commit list: position, subject, status
  chip, revision count, comment/draft/unresolved counts, an "updated since
  your review (1→2)" badge when `last_reviewed_revision < revision`.
  Orphaned changes render collapsed at the bottom (comments preserved).
  `last_scan_error` shows as a banner. Click → change view.
- `/changes/:id` **Review** (the core) —
  - header: subject, chain breadcrumb, base info;
  - chain strip: inline at the right end of the header's meta line
    (commit/parent/age) — one status dot per change in chain order (the
    current change ringed, siblings click through; same dot pattern as the
    dashboard) and an `N/M ▾` toggle. Expanding lists the chain (position,
    subject, unresolved count, status chip per change) in normal flow,
    pushing the content below down; navigating to another change closes it;
  - diff range: Gerrit-style dropdown pair in the diffbar, `Base|rM → rN`.
    The right select is the revision under review — the diff's TO/new
    column and the revision new comments anchor to, tracked in the
    `revision` URL param (it drives the `/revisions/{n}/diff` path, not a
    comment-rendering query). The left
    select drives `?against=`: `base` is an explicit full diff vs parent,
    an earlier `rM` an interdiff `rM → rN` (later revisions shown
    disabled). Default when `last_reviewed_revision` exists and is behind:
    the interdiff `last_reviewed → latest` with a "changes since your
    review" hint; otherwise Base → latest. Each `rN` option is tagged with
    that revision's own comment-thread count (`r2 · 3 comments`, plain text
    — native `<option>`s carry no markup), so discussion is visible before
    switching range; the count includes the reviewer's drafts and folds
    replies into their thread;
  - the diff column and file rail start with a synthetic "Commit message"
    file (`/COMMIT_MSG`, docs/api.md), commentable like code — the full
    message lives there, not in the header;
  - file list (left rail): titled with the diff totals — file count and
    summed +/- counts, both excluding `/COMMIT_MSG` (the message is not a
    file); then per file: path, status letter, +/- counts; selecting
    expands the file section and scrolls to it (the scroll is issued only
    after the expansion has rendered, so it lands right with other files
    collapsed); while scrolling, the rail highlights the file under the
    sticky chrome (scroll spy) and keeps that item visible in the rail's
    own scrollport; all files render in one scroll column (diffshub
    style), unified ⇄ side-by-side toggle persisted in localStorage;
  - file sections are collapsible (header click toggles) and start
    collapsed — except the commit message, the entry point of a commit
    review. The file header carries an `N comments` tally beside its +/-
    counts: the threads visible for that file in the shown range (placement
    rules above — a thread pinned to a hidden revision is not counted), so
    the example `M bla.rs +32 −16 3 comments` omits a comment on a revision
    neither side displays. Collapsed files hide their inline threads; that
    header tally and the rail's draft/comment counts still signal them. The
    rail title carries an expand-all ⇄ collapse-all toggle; `[`/`]` file nav
    reveals like a rail click. Collapse state resets per diff
    (change/revision/base);
  - diff: monospace, full-width gutters with old/new line numbers, add/del
    coloring, per-line syntax highlighting (language from extension; skip
    silently when unknown), hunk separators showing skipped ranges;
  - comments: select diff text — partial within a line or across lines,
    one side at a time (a split-view drag locks to the column it starts
    in) — and press `c` → the editor opens under the selection's last
    line with the range recorded (docs/api.md "Range comments"); `c` on a
    collapsed caret comments its line. Either column is commentable: the
    new column anchors to the selected revision; the old column anchors to
    its parent (base) or, in an interdiff, to the FROM revision's own side
    (`lib/comments.ts` maps the column to the stored `(revision, side)`).
    When `c` cannot map the selection (sides disagree, a hunk gap, a
    cross-file sweep), a transient notice in the diffbar says why. The
    selection-to-range mapping lives in `lib/selection.ts` against
    DiffFileView's data attributes. Comments place by the **diff range**
    (docs/api.md "Comment placement"): a comment shows only when its
    `(revision, side)` is one of the two displayed sides — new-side
    threads under the right/new column, old-side under the left/old column
    (in side-by-side), and not at all when its revision is neither FROM nor
    TO. Ranged threads tint their selected text amber on the matching
    column; the open editor's pending selection tints brighter. Published
    comments render as threads (replies via `parent_id`, author chrome for
    reviewer/agent, resolve toggle) under their anchored line; a comment on
    a displayed side but outside the rendered hunks groups at the top of
    its file with its `line_text` excerpt; drafts get a dashed border +
    `DRAFT` tag and edit/delete;
  - review bar (sticky bottom): draft count, unresolved count, and a
    `Review (a)` button (shortcut `a`) opening the reply modal:
    cover-message textarea, buttons `Approve` / `Request changes` /
    `Comment` → POST review, then navigate to the next pending change in
    the chain (or back to the chain). Escape / backdrop click close it
    (confirm before discarding a typed message; drafts live server-side
    and are kept). On a 409 (agent pushed meanwhile): the modal stays
    open, keeps the cover message and drafts, refetches, shows "new
    revision landed", re-offers submit.
- 404/error states: plain message + link home. Loading: skeleton rows, no
  spinner-only screens.

## Design language

Expert-dense, dark-first (single dark theme for v1). Background `#0d1117`-ish,
mono font for code/shas (`ui-monospace` stack), sans for chrome. Amber =
needs reviewer, blue = agent working, green = approved/ready, red =
changes requested/deletions; gray = informational (the `PARTIAL` badge is not
a call to action — amber stays reserved for "needs reviewer").
Compact paddings; no marketing fluff; every
piece of chrome must earn its pixels. Keyboard shortcuts (`[`/`]` file nav,
`n`/`p` change nav, `c` comment on selection, `a` reply modal) are welcome
but optional in v1.

## Mock mode (UI work without a backend)

`VITE_MOCK=1 npm run dev` makes `client.ts` serve canned fixtures from
`web/src/api/fixtures.ts` (a realistic chain: 3 changes, one with 2
revisions, drafts, published thread, binary + renamed files in the
diff). Keep fixtures contract-true; they double as component-test data.

## Checking your work

`npm run check` (tsc), `npm run build` and `npm test` (vitest; jsdom +
testing-library, against the mock fixtures) must pass inside the devShell.
Visual verification happens through the screenshot harness — see
[dev.md](dev.md).
