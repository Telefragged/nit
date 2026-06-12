#!/usr/bin/env bash
# End-to-end check of the full agent <-> reviewer loop against a throwaway
# repo, exercising the real binary: push --partial -> review -> wait ->
# reply -> fixup -> new revision -> approve -> ready -> merge -> chain
# leaves the dashboard.
#
# Usage: scripts/e2e.sh [nit-binary]     (default: ./result/bin/nit)
# Run inside the devShell: nix develop -c scripts/e2e.sh

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
NIT=${1:-"$ROOT/result/bin/nit"}
[[ -x $NIT ]] || { echo "no nit binary at $NIT (nix build first?)" >&2; exit 1; }
NIT=$(readlink -f "$NIT")

PORT=8917
SERVER=http://127.0.0.1:$PORT
TMP=$(mktemp -d)
SRV_PID=
trap '[[ -n $SRV_PID ]] && kill $SRV_PID 2>/dev/null; rm -rf "$TMP"' EXIT

say()  { printf '== %s\n' "$*"; }
fail() { echo "E2E FAIL: $*" >&2; exit 1; }
# jqe <json> <filter> <expected>
jqe()  {
  local got
  got=$(jq -r "$2" <<<"$1")
  [[ $got == "$3" ]] || fail "$2 = '$got', want '$3'"
}

say "fixture repo"
REPO=$TMP/repo
git init -q -b main "$REPO"
cd "$REPO"
git config user.name e2e
git config user.email e2e@test
echo base > file.txt
git add . && git commit -qm "base"
git checkout -qb feat/demo
printf 'def greet(name):\n    print("hello " + name)\n' > greet.py
git add . && git commit -qm "add greet module

Change-Id: Ie2e0000000000000000000000000000000000001"
# Separate file: a fixup to greet.py must fold (and later autosquash)
# without touching this change's context.
printf 'def farewell(name):\n    print("bye " + name)\n' > farewell.py
git add . && git commit -qm "add farewell module

Change-Id: Ie2e0000000000000000000000000000000000002"

say "server up"
"$NIT" serve --listen 127.0.0.1:$PORT --db "$TMP/nit.sqlite3" &
SRV_PID=$!
for _ in $(seq 50); do
  curl -sf $SERVER/api/health >/dev/null 2>&1 && break
  sleep 0.1
done
curl -sf $SERVER/api/health >/dev/null || fail "server did not come up"

say "agent: push --partial registers the chain as partial"
CHAIN=$("$NIT" push --partial --server $SERVER)
jqe "$CHAIN" .state waiting_for_review
jqe "$CHAIN" .partial true
jqe "$CHAIN" '.changes | length' 2
jqe "$CHAIN" '.changes[0].status' pending
CHAIN_ID=$(jq -r .id <<<"$CHAIN")
CH1=$(jq -r '.changes[0].id' <<<"$CHAIN")

say "reviewer: draft a line comment, request changes"
curl -sf -X POST $SERVER/api/changes/$CH1/drafts -H content-type:application/json \
  -d '{"revision":1,"file":"greet.py","line":2,"side":"new","body":"use an f-string"}' >/dev/null
curl -sf -X POST $SERVER/api/changes/$CH1/reviews -H content-type:application/json \
  -d '{"revision":1,"verdict":"request_changes","message":"one nit"}' >/dev/null

say "agent: wait returns actionable feedback"
FB=$("$NIT" wait --timeout 10 --server $SERVER)
jqe "$FB" .state agents_turn
jqe "$FB" .actionable true
jqe "$FB" '.changes[0].review.verdict' request_changes
jqe "$FB" '.changes[0].comments[0].body' "use an f-string"
COMMENT_ID=$(jq -r '.changes[0].comments[0].id' <<<"$FB")

say "agent: reply --resolve, fix with a fixup!, push"
"$NIT" reply "$COMMENT_ID" --resolve -m "switched to an f-string" --server $SERVER >/dev/null
sed -i 's/print("hello " + name)/print(f"hello {name}")/' greet.py
git add . && git commit -q --fixup="$(git rev-parse HEAD~1)"
CHAIN=$("$NIT" push --server $SERVER)
jqe "$CHAIN" .partial true   # plain push leaves the sticky flag alone
jqe "$CHAIN" '.changes[0].revision' 2
jqe "$CHAIN" '.changes[0].status' pending
jqe "$CHAIN" '.changes[0].needs_rebase' false
jqe "$CHAIN" '.changes[0].counts.unresolved' 0

say "reviewer: comment from revision 1 ports to revision 2"
DETAIL=$(curl -sf "$SERVER/api/changes/$CH1?revision=2")
jqe "$DETAIL" '.comments[0].outdated' true   # the commented line itself changed

say "reviewer: approve every change at its latest revision"
CHAIN=$(curl -sf $SERVER/api/chains/$CHAIN_ID)
while read -r row; do
  id=$(jq -r .id <<<"$row"); rev=$(jq -r .revision <<<"$row")
  curl -sf -X POST $SERVER/api/changes/$id/reviews -H content-type:application/json \
    -d "{\"revision\":$rev,\"verdict\":\"approve\",\"message\":\"lgtm\"}" >/dev/null
done < <(jq -c '.changes[]' <<<"$CHAIN")

say "agent: all approved but partial — still agents_turn, not mergeable"
FB=$("$NIT" status --server $SERVER)
jqe "$FB" .state agents_turn
jqe "$FB" .chain.partial true

say "agent: ready clears partial; state is ready_to_merge"
CHAIN=$("$NIT" ready --server $SERVER)
jqe "$CHAIN" .partial false
jqe "$CHAIN" .state ready_to_merge
FB=$("$NIT" status --server $SERVER)
jqe "$FB" .state ready_to_merge

say "agent: autosquash, ff-merge into main"
GIT_EDITOR=true git rebase --autosquash main -q
git checkout -q main
git merge -q --ff-only feat/demo

say "chain leaves the dashboard after the next scan"
sleep 2.5   # outlast the per-chain scan throttle so the GET rescans
LIST=$(curl -sf $SERVER/api/chains)
jqe "$LIST" '.chains | length' 0
LIST=$(curl -sf "$SERVER/api/chains?status=all")
jqe "$LIST" '.chains[0].status' merged

echo
echo "E2E OK"
