//! `nit wait` over the websocket: it drains the log, waking on any new entry,
//! and parks on the stream until fresh activity lands (docs/agent-workflow.md).

mod common;

use std::time::Duration;

use common::{GitRepo, TestServer, http_get, http_post, http_put, msg, nit, nit_bounded};
use serde_json::{Value, json};

/// `nit push` from the cwd HEAD, returning its `PushResult`.
fn push_head(server: &TestServer, g: &GitRepo) -> Value {
    let workdir = g.workdir();
    let (ok, res, err) = nit(
        server,
        g,
        &[
            "push",
            "--repo",
            workdir.to_str().unwrap(),
            "--branch",
            "feat",
        ],
    );
    assert!(ok, "push failed: {err}");
    res
}

/// `nit wait 0` wakes immediately on any existing activity past the cursor
/// (here, the agent's own push revision).
#[test]
fn wait_returns_existing_activity() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    push_head(&server, &g);

    let (ok, out, err) = nit_bounded(&server, &g, &["wait", "0"], Duration::from_secs(15));
    assert!(ok, "wait failed: {err}");
    let entries = out["entries"].as_array().expect("entries");
    assert!(
        entries.iter().any(|e| e["kind"] == "revision"),
        "wait surfaced the revision: {out}"
    );
    assert!(
        out["cursor"].as_u64().unwrap() > 0,
        "cursor advanced: {out}"
    );
}

/// Parked at the current head, `nit wait` blocks until a reviewer entry lands
/// over the stream, then wakes with it.
#[test]
fn wait_blocks_then_wakes_on_a_review() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let res = push_head(&server, &g);
    let change_id = res["tip_change"]["change_id"].as_u64().unwrap();

    // The head seq after the push (the agent's revision entry).
    let (_, log) = http_get(&server.url(&format!("/api/chains/{change_id}/log")));
    let head_seq = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["seq"].as_u64())
        .max()
        .unwrap();

    // A reviewer stages request_changes and submits the chain shortly after the
    // wait parks (owned URLs so the thread needs no borrow of `server`). The
    // stage is a side-table write (no log entry); the submit appends the
    // `review` that wakes the parked wait.
    let decision_url = server.url(&format!("/api/changes/{change_id}/decision"));
    let submit_url = server.url(&format!("/api/chains/{change_id}/submit"));
    let reviewer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let (st, _) = http_put(
            &decision_url,
            &json!({"decision": "request_changes", "message": "fix the unwrap"}),
        );
        assert_eq!(st, 200);
        let (st, _) = http_post(&submit_url, &json!({}));
        assert_eq!(st, 200);
    });

    let (ok, out, err) = nit_bounded(
        &server,
        &g,
        &["wait", &head_seq.to_string()],
        Duration::from_secs(20),
    );
    reviewer.join().unwrap();
    assert!(ok, "wait failed: {err}");
    let entries = out["entries"].as_array().expect("entries");
    assert!(
        entries
            .iter()
            .any(|e| e["kind"] == "review" && e["payload"]["verdict"] == "request_changes"),
        "wait woke on the review: {out}"
    );
    assert_eq!(out["feedback"]["state"], "agents_turn");
}
