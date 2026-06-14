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

#[test]
fn follow_reviewer_only_suppresses_agent_echoes() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // The push leaves `revisions@0` in the backlog — an agent echo that
    // `--reviewer-only` must drop.
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    let chain_id = chain["id"].as_u64().unwrap();
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args([
            "log",
            "--follow",
            "--reviewer-only",
            "--oneline",
            "--chain",
            &chain_id.to_string(),
            "0",
        ])
        .env("NIT_SERVER", &server.base)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

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

    // A reviewer verdict lands. The first line the monitor emits must be
    // this `review@1`: the backlog `revisions@0` was dropped from the
    // output, so the review is the first thing relayed.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "nit"}),
    );
    assert_eq!(st, 200);
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("reviewer-only follow should stream the review within 10s");
    assert!(
        line.starts_with("1\treview"),
        "first relayed line: {line:?}"
    );

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn follow_reviewer_only_suppresses_a_live_echo() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // Plain push: `revisions@0`, chain not partial.
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    let chain_id = chain["id"].as_u64().unwrap();
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args([
            "log",
            "--follow",
            "--reviewer-only",
            "--oneline",
            "--chain",
            &chain_id.to_string(),
            "0",
        ])
        .env("NIT_SERVER", &server.base)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

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

    // A LIVE agent echo arrives on the stream after connect: flipping the
    // chain to partial appends `partial@1`. It must be suppressed, never
    // relayed — live suppression is the whole point of --reviewer-only.
    let (ok, _, stderr) = nit_register(&server, &g, "push", "feat", &["--partial"]);
    assert!(ok, "{stderr}");

    // Then a reviewer verdict. The first (and only) line the monitor emits
    // must be that `review@2` — proving the live `partial@1` was dropped
    // from the output, not just the backlog `revisions@0`.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "nit"}),
    );
    assert_eq!(st, 200);
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("reviewer-only follow should stream the review within 10s");
    assert!(
        line.starts_with("2\treview"),
        "first relayed line: {line:?}"
    );

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn follow_reviewer_only_holds_back_a_noncompleting_approve() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    let c2 = g.commit(&[c1], &msg("core: add b", "Ib"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // Two changes: approving one cannot make the whole chain `approved`.
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    let chain_id = chain["id"].as_u64().unwrap();
    let change_a = chain["changes"][0]["id"].as_i64().unwrap();
    let change_b = chain["changes"][1]["id"].as_i64().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args([
            "log",
            "--follow",
            "--reviewer-only",
            "--oneline",
            "--chain",
            &chain_id.to_string(),
            "0",
        ])
        .env("NIT_SERVER", &server.base)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

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

    // A comment-less approve of change A only — it leaves the chain short of
    // `approved`, so --reviewer-only must hold it back, exactly like nit wait.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_a}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": ""}),
    );
    assert_eq!(st, 200);

    // request_changes on B always wakes. It must be the first relayed line —
    // proving the non-completing approve@1 was suppressed. (B's verdict keeps
    // the chain off `approved`, so the assertion holds regardless of which
    // entry the follower processes first.)
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_b}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "nit"}),
    );
    assert_eq!(st, 200);
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("reviewer-only follow should stream the request_changes within 10s");
    assert!(
        line.starts_with("2\treview"),
        "first relayed line: {line:?}"
    );

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn follow_reviewer_only_relays_a_chain_completing_approve() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // One change: a comment-less approve completes the chain (state `approved`).
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    let chain_id = chain["id"].as_u64().unwrap();
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args([
            "log",
            "--follow",
            "--reviewer-only",
            "--oneline",
            "--chain",
            &chain_id.to_string(),
            "0",
        ])
        .env("NIT_SERVER", &server.base)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

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

    // Unlike a non-completing approve, a comment-less approve that takes the
    // chain to `approved` IS relayed — it is the wake the agent acts on.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": ""}),
    );
    assert_eq!(st, 200);
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("reviewer-only follow should stream the completing approve within 10s");
    assert!(
        line.starts_with("1\treview"),
        "first relayed line: {line:?}"
    );

    child.kill().unwrap();
    child.wait().unwrap();
}
