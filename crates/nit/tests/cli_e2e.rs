//! End-to-end CLI: the real `nit` binary (`CARGO_BIN_EXE`) run from inside
//! a fixture repo against a real server — push / status / wait / reply
//! per docs/agent-workflow.md.

mod common;

use std::process::Command;

use common::{GitRepo, TestServer, http_post, msg, nit, nit_register};
use serde_json::{Value, json};

#[test]
fn push_wait_status_reply_loop() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap(); // the agent's checkout
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // push: repo + branch passed explicitly, base defaults to main.
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
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

    // wait 0: returns immediately with the whole backlog since the start and
    // the current (not-actionable) state — cursor 0 is behind head, so it
    // does not block.
    let (ok, resp, stderr) = nit(&server, &g, &["wait", "0"]);
    assert!(ok, "{stderr}");
    assert_eq!(resp["feedback"]["state"], "waiting_for_review");
    assert_eq!(resp["head"], 1, "{resp}");
    assert_eq!(resp["entries"].as_array().unwrap().len(), 1, "{resp}");
    assert_eq!(resp["entries"][0]["kind"], "revisions");

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

    // wait 0 now returns the WHOLE run since the cursor — the initial
    // `revisions` push *and* the reviewer's `review` — not just the first
    // entry, with the actionable feedback.
    let (ok, resp, stderr) = nit(&server, &g, &["wait", "0"]);
    assert!(ok, "{stderr}");
    assert_eq!(resp["feedback"]["state"], "agents_turn");
    assert_eq!(resp["feedback"]["actionable"], true);
    assert_eq!(resp["head"], 2, "{resp}");
    let entries = resp["entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "wait must drain the whole backlog: {resp}"
    );
    assert_eq!(entries[0]["kind"], "revisions");
    assert_eq!(entries[0]["idx"], 0);
    assert_eq!(entries[1]["kind"], "review");
    assert_eq!(entries[1]["idx"], 1);
    assert_eq!(
        resp["feedback"]["changes"][0]["comments"][0]["id"].as_i64(),
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
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
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
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &[]);
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
    let (ok, chain, stderr) = nit_register(&server, &g, "push", "feat", &["--partial"]);
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

    // All approved but still partial: agents_turn, never approved.
    let (ok, feedback, stderr) = nit(&server, &g, &["status"]);
    assert!(ok, "{stderr}");
    assert_eq!(feedback["state"], "agents_turn");
    assert_eq!(feedback["chain"]["partial"], true);

    // ready clears the flag; the approved chain becomes mergeable.
    let (ok, chain, stderr) = nit_register(&server, &g, "ready", "feat", &[]);
    assert!(ok, "{stderr}");
    assert_eq!(chain["partial"], false);
    assert_eq!(chain["state"], "approved");
}

#[test]
fn cli_errors_are_human_readable() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();

    // No server listening: a well-formed push (repo path resolves) still
    // reaches the HTTP layer and reports the unreachable server.
    let dead = TestServer::start(g.dir.path().join("dead.sqlite3"), None);
    let base = dead.base.clone();
    drop(dead);
    let workdir = g.workdir();
    let out = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args([
            "push",
            "--repo",
            workdir.to_str().unwrap(),
            "--branch",
            "feat",
        ])
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

// push has no cwd fallback: repo and branch are required, so being inside a
// git checkout is not enough — clap rejects the call before any HTTP, which
// is what stops a stray push from forking a duplicate chain off the wrong
// path.
#[test]
fn push_requires_repo_and_branch() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let out = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["push"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &server.base)
        .output()
        .unwrap();
    assert!(!out.status.success(), "bare push must not fall back to cwd");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--repo"), "{stderr}");
    assert!(stderr.contains("--branch"), "{stderr}");
}

// A new revision of an existing change (amend → new sha, same Change-Id) is a
// fresh `revisions` entry; one `nit wait` from behind it must surface it, not
// stop at an earlier entry. Regression: `wait` used to return only the first
// waking frame, leaving later entries — the new revision among them — stuck in
// the backlog until a subsequent wait.
#[test]
fn wait_surfaces_a_new_revision_behind_an_earlier_entry() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // push r1 of change Ia -> revisions@0
    let (ok, _c, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    // amend (same Change-Id Ia, new content -> new sha) = r2; push -> revisions@1
    let c1b = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nB\n")]);
    g.branch("feat", c1b);
    let (ok, _c, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");

    // One wait from cursor 0 returns the whole run, the new revision included.
    let (ok, resp, stderr) = nit(&server, &g, &["wait", "0"]);
    assert!(ok, "{stderr}");
    assert_eq!(resp["head"], 2, "{resp}");
    let entries = resp["entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "wait must drain the whole backlog: {resp}"
    );
    assert_eq!(entries[1]["idx"], 1);
    assert_eq!(entries[1]["payload"]["added"][0]["number"], 2);
    assert_eq!(entries[1]["payload"]["added"][0]["change_key"], "Ia");
}
