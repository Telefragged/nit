#!/usr/bin/env bash
# Runs `nix flake check` on each commit in BASE..HEAD, several at a time,
# then prints ok or FAIL for each commit, oldest first. Exits 0 only when
# every commit passes. Usage: scripts/check-commits.sh <base> [jobs]
#
# CI checks every commit of a PR, not only the tip. Each check builds the
# commit as a flake ref, so it leaves the checkout alone. The log of each
# commit goes to .git/check-commits/<sha>.log.
set -uo pipefail
base=${1:?usage: scripts/check-commits.sh <base> [jobs]}
jobs=${2:-8}
repo=$(git rev-parse --show-toplevel)
logs=$(git rev-parse --path-format=absolute --git-common-dir)/check-commits
mkdir -p "$logs"

revs=$(git rev-list --reverse "$base..HEAD")
for rev in $revs; do
  rm -f "$logs/$rev.ok"
done

echo "$revs" | xargs -P "$jobs" -I{} sh -c \
  'nix flake check --keep-going "git+file://$1?rev=$2" >"$3/$2.log" 2>&1 && touch "$3/$2.ok"' \
  _ "$repo" {} "$logs"

status=0
for rev in $revs; do
  subject=$(git log -1 --format='%h %s' "$rev")
  if [ -e "$logs/$rev.ok" ]; then
    echo "ok    $subject"
  else
    echo "FAIL  $subject  ($logs/$rev.log)"
    status=1
  fi
done
exit "$status"
