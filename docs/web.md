# Web frontend

## Screenshot harness (frontend checking for AI agents)

Agents read PNGs, not browsers. The harness renders every UI state from
fixtures with no backend, writing `screenshots/*.png` (repo root,
gitignored); run it and `Read` them:

```sh
cd web && nix develop -c npm run screenshots
```

It covers detailed states (drafts, 409s, interdiff); add a capture with
every new page or state. Each capture also writes `screenshots/<name>.html`:
the same state as one file, with its CSS inlined and its scripts removed.
Attach it to a UI change so that the reviewer reads the page in nit:

```sh
nit attach screenshots/review-attachments.html --change <number>
```

The npm `@playwright/test` version must match
`pkgs.playwright-driver` (the devShell sets `$PLAYWRIGHT_DRIVER_VERSION`) —
the `web-screenshots` flake check runs it in the sandbox and its
output _is_ the PNGs, so `nix build .#checks.<system>.web-screenshots` both
catches a skew and gives you the PNGs in `result/`.

## Component gallery

The gallery renders one component at a time, in a state that a story
defines. A story is a named export of a `*.story.tsx` file next to
its component under `web/src/`. It is a plain React component that renders
the component with fixed data, and with the providers and state that the
component needs. Its id is the path of the file under `src/` without
`.story.tsx`, then the export name, for example
`components/diff/FileRail/Default`.

Run the specs and capture every story:

```sh
cd web && nix develop -c npm run gallery
```

The run writes `screenshots/gallery/<id>.png` in the dark theme,
`screenshots/gallery/<id>-light.png` in the light theme, and
`screenshots/gallery/<id>.html`. The page contains the whole gallery, with
every script and stylesheet inline. The page renders its story on load.
nit shows an attached page in a sandboxed frame, and the scripts of the
page run in that frame. Attach the page so that the reviewer can click the
component in nit:

```sh
nit attach screenshots/gallery/components/diff/FileRail/Default.html --change <number>
```

The specs are Playwright tests in `web/gallery/*.spec.ts`. The `mount`
fixture of `@playwright/test` opens the gallery, renders a story, and
returns a locator for its root. `mount` returns after React commits the
story, so a spec acts on the page at once:

- Check an element with `one()`, and click it with `clickNow()`
  (`web/gallery/act.ts`). Both fail at once when the locator matches no
  element or several.
- Fetch a value once, then assert on it with `node:assert/strict`. ESLint
  reports an error for the `expect` of Playwright and for a Playwright wait,
  because they retry until a timeout (golden rule 13).

The `web-gallery` flake check runs `npm run gallery` in the sandbox. The
output of the check is the PNGs and the pages. `npm run dev` also serves the
gallery at `/gallery/`. To render a story there, call
`mount({ story: "<id>" })` in the browser console.
