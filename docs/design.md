# Design

The rules for the look of the web UI. `web/src/styles/tokens.css` defines the tokens that the rules name.

## Color

- **Yellow is the reviewer's work.** Use the `--color-mark` tokens for a draft, an open thread, a commented range, the count of either, and the pending status. Use them for nothing else, so that a yellow element always needs the reviewer.
- **Chrome is neutral.** Bars, borders, text, the current row and a linked line use the neutral tokens. A link has the color of the text. A link in prose also has an underline.
- **Four hues are diff data.** Green and red are the additions and deletions of the author. Blue and purple are rebase drift: the lines that a rebase added and removed.
- **A change status reuses three of them.** Approved is green, changes requested is red, and merged is purple. Pending is yellow. Every other status is gray.
- **Syntax colors are quiet.** A keyword is violet, a string or a literal is teal, and a comment is gray. No token is red, orange, yellow or green, so no token has the color of a deletion, a mark or an addition.

## Themes

Each color token has a light and a dark value, and the system setting selects one. A rule takes its color from a token and never from a literal, so that it holds in both themes.

The screenshot harness and the gallery capture the dark theme. A capture with `colorScheme: "light"` and each `<id>-light.png` of the gallery show the light theme (`docs/web.md`).

## Type

The interface uses Atkinson Hyperlegible Next. Code, paths, commit ids and revision numbers use Intel One Mono. Both families are drawn so that similar characters differ, for example `0` and `O`.
