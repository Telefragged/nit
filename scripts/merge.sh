#!/usr/bin/env bash
# The approve action: merge an approved nit chain onto main. Run from the
# chain's worktree once `nit status` reports the chain `approved`.
#
# Rebases onto main if main moved, runs `nix flake check` on every commit,
# then fast-forward-merges into main (no merge commits — main stays linear).
# The happy path prints one line per step; any failure prints git's own output
# plus what to do next, then exits non-zero.
#
# Usage: nix develop -c scripts/merge.sh     (from inside .worktrees/<slug>)
#
# Covers the no-conflict case. A rebase that stops on a merge conflict hands
# you the repo mid-rebase to fix and continue; a failing check names every
# commit that failed and leaves the branch where it is — see the `merge` skill.

set -euo pipefail

base=main
cd "$(git rev-parse --show-toplevel)"

if git merge-base --is-ancestor HEAD "$base"; then
  echo "HEAD is already on $base — nothing to merge" >&2
  exit 1
fi

# 1. Rebase onto $base only if it moved. Quiet when HEAD is already on top.
if ! git merge-base --is-ancestor "$base" HEAD; then
  if ! out=$(git rebase "$base" 2>&1); then
    echo "$out" >&2
    exit 1
  fi
  echo "branch rebased"
fi

# 2. Check every commit. HEAD sits on top of $base now, so each commit is a
#    flake ref away and needs no checkout — they run concurrently, and a whole
#    chain's failures come back from one run instead of one per re-run. Bounded
#    because each check is a nix evaluation of its own, and a long chain would
#    otherwise start one per commit at once. `--keep-going` so a commit's log
#    holds every check that failed, not only the first.
revs=$(git rev-list --reverse "$base..HEAD")
# One directory per run, under the git dir rather than a worktree or $TMPDIR:
# a failing run's logs outlive the shell that produced them, and a re-run
# leaves the previous run's alongside instead of over.
logs="$(git rev-parse --path-format=absolute --git-common-dir)/merge-checks/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$logs"
for rev in $revs; do
  while (($(jobs -rp | wc -l) >= ${NIT_MERGE_JOBS:-8})); do wait -n || true; done
  (nix flake check --keep-going "git+file://$PWD?rev=$rev" >"$logs/$rev.log" 2>&1 &&
    touch "$logs/$rev.ok") &
done
wait

failed=()
for rev in $revs; do
  [ -e "$logs/$rev.ok" ] || failed+=("$rev")
done

if ((${#failed[@]})); then
  echo "flake check failed on ${#failed[@]} of $(wc -w <<<"$revs") commits:" >&2
  for rev in "${failed[@]}"; do
    git log -1 --format='  %h %s' "$rev" >&2
    echo "    $logs/$rev.log" >&2
  done
  echo >&2
  echo "amend each fix into the commit above it, 'nit push', then re-run this script" >&2
  exit 1
fi
rm -rf "$logs"
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
