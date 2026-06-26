## Static UI

Everything outside `/api` serves the built SPA (`--web-dist`/`$NIT_WEB_DIST`),
falling back to `index.html` for client-side routes (`/repos/1`,
`/changes/10`).

Every static response carries `Cache-Control: max-age=60`, so a browser holds
an asset for at most a minute and picks up a redeployed UI promptly.
