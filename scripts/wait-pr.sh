#!/usr/bin/env bash
# Waits until a PR is no longer open, or until it cannot merge as it is:
# a check failed, or the branch is behind its base or conflicts with it.
# Exits 0 only when the PR merged. Usage: scripts/wait-pr.sh <pr>
#
# It polls because GitHub offers no event that a local shell can wait on.
# It reads the PR state, not the checks, because `gh pr checks --watch`
# exits with "no checks reported" before the first check registers.
set -uo pipefail
pr=${1:?usage: scripts/wait-pr.sh <pr>}

while pr_state=$(gh pr view "$pr" --json state,mergeStateStatus \
  --jq '.state + " " + .mergeStateStatus'); do
  read -r state merge <<<"$pr_state"
  [ "$state" = OPEN ] || break
  case "$merge" in
  BEHIND)
    echo "PR $pr: the branch is behind its base, so rebase it" >&2
    exit 1
    ;;
  DIRTY)
    echo "PR $pr: the branch conflicts with its base, so rebase it" >&2
    exit 1
    ;;
  esac
  if gh pr checks "$pr" --json bucket --jq '.[].bucket' 2>/dev/null | grep -qx fail; then
    echo "PR $pr: a check failed" >&2
    exit 1
  fi
  sleep 10
done
echo "PR $pr: ${state:-unknown}"
[ "${state:-}" = MERGED ]
