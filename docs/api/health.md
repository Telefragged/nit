## Health

- `GET /api/health` → `{"status":"ok","version":"0.1.0+a1b2c3d4e5f6"}`

`version` is the running server's build: the crate semver, plus `+<sha>` (and
`.dirty` if built from a modified tree) when built from a git tree — a bare
`0.1.0` from a revless tarball. `nit --version` reads it to report the server's
build and reachability (the canonical "is nit up" check).
