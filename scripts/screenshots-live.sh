#!/usr/bin/env bash
# Capture screenshots of the REAL server (not mock fixtures): seeds a demo
# repo with chains in every dashboard state, serves the built UI, and runs
# the playwright harness against it. Output: screenshots/live-*.png.
#
# Usage: scripts/screenshots-live.sh [nit-binary]   (default: ./result/bin/nit)
# Run inside the devShell: nix develop -c scripts/screenshots-live.sh
# (web/node_modules must exist: cd web && npm install)

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
NIT=${1:-"$ROOT/result/bin/nit"}
[[ -x $NIT ]] || {
  echo "no nit binary at $NIT (nix build first?)" >&2
  exit 1
}
NIT=$(readlink -f "$NIT")

PORT=8923
SERVER=http://127.0.0.1:$PORT
TMP=$(mktemp -d)
SRV_PID=
trap '[[ -n $SRV_PID ]] && kill $SRV_PID 2>/dev/null; rm -rf "$TMP"' EXIT

commit_change() { # <file> <content> <subject> <change-id-suffix>
  printf '%s' "$2" >"$1"
  git add "$1"
  git commit -qm "$3

Change-Id: Idemo00000000000000000000000000000000$4"
}

git init -q -b main "$TMP/repo"
cd "$TMP/repo"
git config user.name demo
git config user.email demo@nit
printf 'fn main() {\n    println!("hello");\n}\n' >main.rs
git add . && git commit -qm "initial layout"

# Chain A — two commits, untouched by review: WAITING FOR REVIEW.
git checkout -qb feat/retry-policy
commit_change retry.rs 'pub struct Retry {
    max_attempts: u32,
}

impl Retry {
    pub fn new(max_attempts: u32) -> Self {
        Self { max_attempts }
    }
}
' "retry: add bounded retry policy" 0001
commit_change backoff.rs 'use std::time::Duration;

pub fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(100 * 2u64.pow(attempt.min(6)))
}
' "retry: exponential backoff helper" 0002

# Chain B — request_changes with line comments: AGENT'S TURN.
git checkout -q main
git checkout -qb fix/log-rotation
commit_change rotate.rs 'pub fn rotate(path: &str) -> std::io::Result<()> {
    let backup = format!("{}.1", path);
    std::fs::rename(path, backup)?;
    Ok(())
}
' "logs: rotate on size threshold" 0003

# Chain C — single approved commit: READY TO MERGE.
git checkout -q main
git checkout -qb chore/pin-toolchain
commit_change rust-toolchain.toml '[toolchain]
channel = "1.95.0"
' "build: pin rust toolchain" 0004

"$NIT" serve --listen 127.0.0.1:$PORT --db "$TMP/nit.sqlite3" &
SRV_PID=$!
for _ in $(seq 50); do
  curl -sf $SERVER/api/health >/dev/null 2>&1 && break
  sleep 0.1
done

"$NIT" push --server $SERVER --repo "$TMP/repo" --branch feat/retry-policy >/dev/null
B=$("$NIT" push --server $SERVER --repo "$TMP/repo" --branch fix/log-rotation)
C=$("$NIT" push --server $SERVER --repo "$TMP/repo" --branch chore/pin-toolchain)

# Chain B: two draft comments + request_changes.
BCH=$(jq -r '.changes[0].id' <<<"$B")
curl -sf -X POST $SERVER/api/changes/$BCH/drafts -H content-type:application/json \
  -d '{"revision":1,"file":"rotate.rs","line":3,"side":"new","body":"This clobbers the previous backup — rotate .1 to .2 first."}' >/dev/null
curl -sf -X POST $SERVER/api/changes/$BCH/drafts -H content-type:application/json \
  -d '{"revision":1,"file":"rotate.rs","line":2,"side":"new","body":"Use PathBuf, not string concatenation."}' >/dev/null
curl -sf -X POST $SERVER/api/changes/$BCH/reviews -H content-type:application/json \
  -d '{"revision":1,"verdict":"request_changes","message":"Backup handling needs work before this can land."}' >/dev/null

# Chain C: approve.
CCH=$(jq -r '.changes[0].id' <<<"$C")
curl -sf -X POST $SERVER/api/changes/$CCH/reviews -H content-type:application/json \
  -d '{"revision":1,"verdict":"approve","message":"lgtm"}' >/dev/null

cd "$ROOT/web"
NIT_BASE_URL=$SERVER node screenshots/capture.mjs
