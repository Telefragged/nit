#!/usr/bin/env bash
# The git-mito on-update hook: builds a new base, so that every worktree
# snapshotted from it starts with warm Rust and web build caches.
# Configure it in .mito.toml:
#
#   [base]
#   on-update = "scripts/mito-on-update.sh"
set -euo pipefail

# Cargo loads the fingerprint files of every unit that a build needs, also
# of a fresh unit. If the access time of a file is older than its
# modification time, the load sets the access time to now, also under
# relatime. So after the builds, a unit whose fingerprint files still have
# access time 0 is stale. On a noatime mount, every unit that the builds
# do not compile is stale.
fingerprints=target/debug/.fingerprint
if [[ -d $fingerprints ]]; then
  find "$fingerprints" -type f -exec touch -a -d @0 {} +
fi

nix develop -c sh -euc '
  cargo check --all-targets
  cargo build --all-targets
  cd web
  npm ci
  gen-wasm
'

# Deletes the stale units. The name of every file and directory of a unit
# ends in the unit hash, as in deps/libsyn-100700be8507aff7.rlib.
declare -A stale
for unit in "$fingerprints"/*; do
  stale[${unit##*-}]=1
done
live=$(find "$fingerprints" -type f -newerat @0 -printf '%h\n')
for unit in $live; do
  unset "stale[${unit##*-}]"
done
for path in "$fingerprints"/* target/debug/{build,deps}/* target/debug/*; do
  name=${path##*/}
  name=${name%%.*}
  if [[ -v stale[${name##*-}] ]]; then
    rm -rf "$path"
  fi
done

# Keeps the most recently compiled crates of the incremental cache, up to
# 5 GiB, and deletes the others. rustc writes a new session into the
# directory of a crate on every compile of that crate, so the newest
# modification time in it is the time of the last compile.
du -sk --time --time-style=+%s target/debug/incremental/* | sort -k2,2nr |
  awk '(total += $1) > 5 * 1024 * 1024 { print $3 }' | xargs -r rm -rf

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
