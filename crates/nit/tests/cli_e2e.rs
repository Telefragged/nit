//! End-to-end CLI: the real `nit` binary (`CARGO_BIN_EXE`) run from inside
//! a fixture repo against a real server — push / status / wait / reply
//! per docs/agent-workflow.md.

mod common;

use std::process::Command;

use common::{GitRepo, TestServer, http_post, msg, nit};
use serde_json::{Value, json};

#[test]
fn push_wait_status_reply_loop() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap(); // the agent's checkout
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // push: repo root + branch from cwd, base defaults to main.
    let (ok, chain, stderr) = nit(&server, &g, &["push"]);
    assert!(ok, "{stderr}");
    assert_eq!(chain["branch"], "feat");
    assert_eq!(chain["base"], "main");
    assert_eq!(chain["state"], "waiting_for_review");
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    // status: feedback snapshot, not actionable yet.
    let (ok, feedback, stderr) = nit(&server, &g, &["status"]);
    assert!(ok, "{stderr}");
    assert_eq!(feedback["state"], "waiting_for_review");
    assert_eq!(feedback["actionable"], false);

    // wait --timeout 1: not actionable, returns the snapshot after ~1s.
    let (ok, feedback, stderr) = nit(&server, &g, &["wait", "--timeout", "1"]);
    assert!(ok, "{stderr}");
    assert_eq!(feedback["state"], "waiting_for_review");

    // Reviewer acts (over HTTP, as the browser would).
    let (st, draft) = http_post(
        &server.url(&format!("/api/changes/{change_id}/drafts")),
        &json!({"revision": 1, "file": "a.txt", "line": 1, "body": "naming"}),
    );
    assert_eq!(st, 200);
    let comment_id = draft["id"].as_i64().unwrap();
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "fix naming"}),
    );
    assert_eq!(st, 200);

    // wait now returns immediately with the actionable feedback.
    let (ok, feedback, stderr) = nit(&server, &g, &["wait"]);
    assert!(ok, "{stderr}");
    assert_eq!(feedback["state"], "agents_turn");
    assert_eq!(feedback["actionable"], true);
    assert_eq!(
        feedback["changes"][0]["comments"][0]["id"].as_i64(),
        Some(comment_id)
    );

    // reply --resolve threads under the root and resolves it.
    let (ok, reply, stderr) = nit(
        &server,
        &g,
        &[
            "reply",
            &comment_id.to_string(),
            "-m",
            "renamed",
            "--resolve",
        ],
    );
    assert!(ok, "{stderr}");
    assert_eq!(reply["author"], "agent");
    assert_eq!(reply["parent_id"].as_i64(), Some(comment_id));
    let (ok, feedback, _) = nit(&server, &g, &["status"]);
    assert!(ok);
    assert_eq!(feedback["changes"][0]["unresolved"], 0);

    // A merge commit on the chain: push prints the chain JSON but exits
    // non-zero with the scan error on stderr.
    let side = g.commit(&[g.root], "side\n", &[("s.txt", "s\n")]);
    let merge = g.commit(&[c1, side], "merge side\n", &[]);
    g.branch("feat", merge);
    let (ok, chain, stderr) = nit(&server, &g, &["push"]);
    assert!(!ok, "merge commits must fail the push");
    assert!(
        chain["last_scan_error"]
            .as_str()
            .unwrap()
            .contains("merge commits"),
        "{chain}"
    );
    assert!(stderr.contains("merge commits"), "{stderr}");

    // Recovery is plain re-push after the agent rebases.
    g.branch("feat", c1);
    let (ok, chain, stderr) = nit(&server, &g, &["push"]);
    assert!(ok, "{stderr}");
    assert_eq!(chain["last_scan_error"], Value::Null);
}

#[test]
fn partial_push_blocks_merge_until_ready() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // push --partial: chain registers partial, review can start.
    let (ok, chain, stderr) = nit(&server, &g, &["push", "--partial"]);
    assert!(ok, "{stderr}");
    assert_eq!(chain["partial"], true);
    assert_eq!(chain["state"], "waiting_for_review");
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    // Reviewer approves the only change (over HTTP, as the browser would).
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": "lgtm"}),
    );
    assert_eq!(st, 200);

    // All approved but still partial: agents_turn, never ready_to_merge.
    let (ok, feedback, stderr) = nit(&server, &g, &["status"]);
    assert!(ok, "{stderr}");
    assert_eq!(feedback["state"], "agents_turn");
    assert_eq!(feedback["chain"]["partial"], true);

    // ready clears the flag; the approved chain becomes mergeable.
    let (ok, chain, stderr) = nit(&server, &g, &["ready"]);
    assert!(ok, "{stderr}");
    assert_eq!(chain["partial"], false);
    assert_eq!(chain["state"], "ready_to_merge");
}

#[test]
fn cli_errors_are_human_readable() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();

    // No server listening.
    let dead = TestServer::start(g.dir.path().join("dead.sqlite3"), None);
    let base = dead.base.clone();
    drop(dead);
    let out = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["push"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &base)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("is 'nit serve' running?"), "{stderr}");

    // wait/status before any push: explain the fix.
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let out = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["status"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &server.base)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("run 'nit push' first"), "{stderr}");
}
