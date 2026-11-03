# Generated files

## Cargo.nix and the npm hash

The crate2nix build file `Cargo.nix` is checked in. After any `Cargo.lock`
change, regenerate it with `nix develop -c crate2nix generate` and commit it
in the same change — a stale `Cargo.nix` fails the build fast. Likewise,
changing `web/package-lock.json` means refreshing the `hash` of
`fetchNpmDeps` in `web/default.nix`
(`nix run nixpkgs#prefetch-npm-deps -- web/package-lock.json`
prints it); a stale hash breaks `nix build` and every
`nix run '…?ref=main#nit'` CLI invocation.

## Wire types

The web's wire types (`web/src/api/types.gen.ts`) are generated the same
way — from `crates/nit-types` via `nix run .#gen-types`, a native
`cargo test` that runs the crate's `ts`-feature `ts-rs` exporter (no wasm).
Regenerate and commit it whenever those types change; the `types-drift`
check fails a stale file.

## WebAssembly fold

The shared change fold is compiled to WebAssembly (`crates/nit-wasm`) for the
event-driven change page. `gen-wasm` writes the glue + `.wasm` into
`web/src/wasm/` — **gitignored** (binary, derived) and injected into every web
nix build. It's on `PATH` inside the devShell (also runnable standalone as
`nix run .#gen-wasm`); every `npm run` frontend script generates it on demand,
so there's no manual step. The `wasm-build` check compiles it under `nix
flake check`. The `wasm-bindgen-cli` in the devShell and `crates/nit-wasm`'s
`wasm-bindgen` dep are pinned to the same version — a skew breaks the glue at
init, not at build.
