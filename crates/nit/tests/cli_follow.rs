//! `nit log --follow`: the real binary streams the event log live — it
//! replays the backlog from the cursor, then tails each new entry as it is
//! appended (a cooperative monitor; docs/agent-workflow.md).

mod common;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::{GitRepo, TestServer, http_post, msg, nit_register};
use serde_json::json;

#[test]
fn follow_streams_backlog_then_new_entries() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // One pushed change leaves `revisions@0` in the log — the backlog the
    // follower must replay on connect.
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    let chain_id = chain["id"].as_u64().unwrap();
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    // Follow from cursor 0 (`--chain` sidesteps cwd resolution); `--oneline`
    // makes every entry one parseable line.
    let mut child = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args([
            "log",
            "--follow",
            "--oneline",
            "--chain",
            &chain_id.to_string(),
            "0",
        ])
        .env("NIT_SERVER", &server.base)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    // Read lines off the child on a thread so a regression times out here
    // instead of hanging the suite.
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(l) = line else { break };
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    let next = |what: &str| {
        rx.recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("follow should stream {what} within 10s"))
    };

    // Backlog replay: the push's revisions entry arrives first.
    let line = next("the backlog entry");
    assert!(line.starts_with("0\trevisions"), "backlog: {line:?}");

    // A new review lands while we follow — it streams as the next entry.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "nit"}),
    );
    assert_eq!(st, 200);
    let line = next("the live review entry");
    assert!(line.starts_with("1\treview"), "tail: {line:?}");

    child.kill().unwrap();
    child.wait().unwrap();
}
