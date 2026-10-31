#!/usr/bin/env bash
# The git-mito on-update hook: builds a new base, so that every worktree
# snapshotted from it starts with warm Rust and web build caches.
# Configure it in .mito.toml:
#
#   [base]
#   on-update = "scripts/mito-on-update.sh"
set -euo pipefail
nix develop -c sh -euc '
  cargo check --all-targets
  cargo build --all-targets
  cd web
  npm ci
  gen-wasm
'

# Every derivation that the flake outputs for this system: the packages,
# checks, dev shells and formatter, and the programs of the apps.
drvs=($(nix eval --impure --raw .#. --apply '
  outputs:
  with builtins;
  let
    drvsOf = v: if v.type or null == "app" then attrNames (getContext v.program) else [ v.drvPath ];
    perSystem =
      o:
      let
        v = o.${currentSystem} or { };
      in
      if v.type or null == "derivation" then [ v.drvPath ] else concatMap drvsOf (attrValues v);
  in
  concatStringsSep "\n" (concatMap perSystem (attrValues outputs))
'))
nix build --no-link "${drvs[@]/%/^*}"

# Roots the build closure of those derivations in .gcroot: their outputs,
# and every dependency output that the builds put in the store. A GC then
# keeps what a rebuild needs, without keep-outputs in nix.conf. The old
# base takes its .gcroot with it, so the previous closure becomes garbage.
closure=$(mktemp)
trap 'rm -f "$closure"' EXIT
nix-store --query --requisites --include-outputs "${drvs[@]}" >"$closure"
closure=$closure nix build --impure --out-link .gcroot --expr '
  with builtins;
  toFile "nit-build-closure" (
    concatStringsSep "\n" (
      map storePath (filter (p: isString p && p != "") (split "\n" (readFile (getEnv "closure"))))
    )
  )
'
