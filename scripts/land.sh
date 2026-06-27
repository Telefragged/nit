#!/usr/bin/env bash
# The approve action: land an approved nit chain onto main. Run from the
# chain's worktree once `nit status` reports the chain `approved`.
#
# Rebases onto main if main moved, runs `nix flake check` on every commit,
# then fast-forward-merges into main (no merge commits — main stays linear).
# The happy path prints one line per step; any failure prints git's own output
# plus what to do next, then exits non-zero.
#
# Usage: nix develop -c scripts/land.sh     (from inside .worktrees/<slug>)
#
# Covers the no-conflict case. A rebase that stops on a merge conflict, or a
# commit that fails the checks, hands you the repo mid-rebase to fix and
# continue — see the `land` skill.

set -euo pipefail

base=main
cd "$(git rev-parse --show-toplevel)"

if git merge-base --is-ancestor HEAD "$base"; then
  echo "HEAD is already on $base — nothing to land" >&2
  exit 1
fi

# 1. Rebase onto $base only if it moved; a pure rebase keeps each commit's
#    patch-id (and its approval). Quiet when HEAD is already on top.
if ! git merge-base --is-ancestor "$base" HEAD; then
  if ! out=$(git rebase "$base" 2>&1); then
    echo "$out" >&2
    exit 1
  fi
  echo "branch rebased"
fi

# 2. Replay every commit through `nix flake check`. HEAD sits on top of $base
#    now, so only a failing check — never a conflict — can stop this, and it
#    leaves HEAD on the offending commit.
if ! out=$(git rebase --exec 'nix flake check' "$base" 2>&1); then
  echo "$out" >&2
  echo "you're now on commit $(git rev-parse --short HEAD) — fix it, 'git rebase --continue', then re-run this script ('git rebase --abort' to bail)" >&2
  exit 1
fi
echo "flake check passed"

# 3. Fast-forward $base in the primary worktree (where it's checked out — the
#    chain worktrees hang off it); never check out $base here.
target=$(git rev-parse HEAD)
primary=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
if ! out=$(git -c advice.diverging=false -C "$primary" merge --ff-only "$target" 2>&1); then
  echo "$out" >&2
  echo "$base moved during checks — re-run this script to rebase onto it and retry" >&2
  exit 1
fi
echo "branch merged into $base"
