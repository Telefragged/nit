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
- **A thread is a comment icon.** A count of threads, and a thread row of the file rail, has the comment icon in the form of its threads: a yellow fill for open threads, a dashed yellow outline for drafts, and gray for resolved threads. A change status is a dot, so the two do not look alike. Where there is room, the word for the form follows the count.
- **The chevron of a file in the rail shows its threads.** It has the yellow fill when the file has an open thread, and the dashed yellow outline when it has drafts only.
- **A resolved thread is one row.** An open thread shows in full. A resolved thread shows its state, the start of its first comment, and the count of its comments. The chevron of a thread changes it to the other form.

## Themes

Each color token has a light and a dark value, and the system setting selects one. A rule takes its color from a token and never from a literal, so that it holds in both themes.

The screenshot harness and the gallery capture the dark theme. A capture with `colorScheme: "light"` and each `<id>-light.png` of the gallery show the light theme (`docs/web.md`).

## Type

The interface uses Atkinson Hyperlegible Next. Code, paths, commit ids and revision numbers use Intel One Mono. Both families are drawn so that similar characters differ, for example `0` and `O`.

A rule uses one of five sizes: 11px for a count or a key, 12px for a label and for code, 13px for text, 15px for a title and for a rendered document, and 18px for the subject of a change. A size inside a rendered document or a comment can be relative to its text.

## Components

A control is a shadcn/ui component in `web/src/components/ui/`. Add one with `npx shadcn add <name>` (`web/components.json`), then reduce it to the variants that a caller uses. Its classes are Tailwind utilities, and `web/src/styles/index.css` limits those utilities to the tokens.

The command writes `import { cn } from "cn"` and adds the package `cn`. Change the import to `@/lib/utils`, and remove the package from `package.json`.

A view has at most one filled yellow button, and that button publishes the work of the reviewer.

A button with a one-key shortcut shows the key in a `Kbd`. The Shortcuts button of the review page lists every key. A `Kbd` shows a letter in uppercase, and each modifier as its word, for example `Shift+N` and `Ctrl+Enter`.

The diff grid and its selection logic are hand-written CSS on the same tokens.
