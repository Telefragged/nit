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
