# Design

The rules for the look of the web UI. `web/src/styles/tokens.css` defines the tokens that the rules name.

## Color

- **Yellow is the reviewer's work.** Use the `--color-mark` tokens for a draft, an open thread, a commented range, the count of either, and the pending status. Use them for nothing else, so that a yellow element always needs the reviewer.
- **Chrome is neutral.** Bars, borders, text, the current row and a linked line use the neutral tokens. A link has the color of the text. A link in prose also has an underline.
- **Four hues are diff data.** Green and red are the additions and deletions of the author. Blue and purple are rebase drift: the lines that a rebase added and removed.
- **A change status reuses three of them.** Approved is green, changes requested is red, and merged is purple. Pending is yellow. Every other status is gray.
- **Syntax colors are quiet.** A keyword is violet, a string or a literal is teal, and a comment is gray. No token is red, orange, yellow or green, so no token has the color of a deletion, a mark or an addition.

## Threads

- **Form shows the state.** A draft has a dashed yellow outline. An open thread has a yellow dot. A resolved thread has a gray check. The box of a published thread is neutral.
- **The gutter shows where the threads are.** The line number of a commented line has the form of its thread: a yellow fill for an open thread, a dashed yellow outline for a draft, and a gray fill for a resolved thread.
- **A count has the form of its threads.** A file header shows one pill for each form, with the word for it: a dashed yellow outline for drafts, a yellow fill for open threads, and a gray fill for resolved threads. In the file rail, the chevron of a file with an open thread has the yellow fill, and the chevron of a file with drafts only has the dashed outline. The comment icon of each thread row under it has the form of its thread.
- **A resolved thread is one row.** An open thread shows in full. A resolved thread shows its state, the start of its first comment, and the count of its comments. The chevron of a thread changes it to the other form.

## Themes

Each color token has a light and a dark value, and the system setting selects one. A rule takes its color from a token and never from a literal, so that it holds in both themes.

The screenshot harness and the gallery capture the dark theme. A capture with `colorScheme: "light"` and each `<id>-light.png` of the gallery show the light theme (`docs/web.md`).

## Type

The interface uses Atkinson Hyperlegible Next. Code, paths, commit ids and revision numbers use Intel One Mono. Both families are drawn so that similar characters differ, for example `0` and `O`.

## Components

A control is a shadcn/ui component in `web/src/components/ui/`. Add one with `npx shadcn add <name>` (`web/components.json`), then reduce it to the variants that a caller uses. Its classes are Tailwind utilities, and `web/src/styles/index.css` limits those utilities to the tokens.

The command writes `import { cn } from "cn"` and adds the package `cn`. Change the import to `@/lib/utils`, and remove the package from `package.json`.

A view has at most one filled yellow button, and that button publishes the work of the reviewer.

The diff grid and its selection logic are hand-written CSS on the same tokens.
