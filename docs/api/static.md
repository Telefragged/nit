## Static UI

Everything outside `/api` serves the built SPA (`--web-dist`/`$NIT_WEB_DIST`),
falling back to `index.html` for client-side routes (`/repos/1`,
`/changes/10`).
