//! End-to-end CLI: the real `nit` binary (`CARGO_BIN_EXE`) run from inside a
//! fixture repo against a real server. The agent drives push / status /
//! log / comment / reopen one-shot (the live followers `nit wait` /
//! `nit log --follow` return in a later stage) — docs/agent-workflow.md.
//!
//! `nit push` walks the change-centric model: oldest-first, upsert each change
//! by its `Change-Id`, append a revision iff the sha moved (revisions are
//! 0-based). `nit status`/`nit log` resolve the cwd's tip change from local
//! HEAD, then read the derived chain on demand.

mod common;

use std::process::Command;

use common::{GitRepo, TestServer, msg, nit, nit_register};

/// `nit push` prints a `PushResult` (`tip_change`) and registers the chain;
/// `nit status`/`nit log` then read the derived chain back, resolved from the
/// cwd HEAD.
#[test]
fn push_prints_result_then_status_and_log_read_it_back() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap(); // the agent's checkout
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
    assert_eq!(push["tip_change"]["revision"], 0, "{push}");
    assert_eq!(push["tip_change"]["status"], "pending");

    // status (no --oneline) reads the derived chain back from
    // `GET /api/chains/{tip}` for the cwd HEAD.
    let (ok, status, stderr) = nit(&server, &g, &["status"]);
    assert!(ok, "{stderr}");
    assert_eq!(status["state"], "waiting_for_review");
    assert_eq!(status["path"].as_array().unwrap().len(), 1);
    assert_eq!(status["path"][0]["position"], 0);
    assert_eq!(status["path"][0]["change_key"], "Ia");
    assert_eq!(status["path"][0]["revision"], 0);

    let out = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["status", "--oneline"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &server.base)
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("state=waiting_for_review"), "{text}");
    assert!(text.contains("\tIa\t"), "one line per member: {text}");

    let (ok, log, stderr) = nit(&server, &g, &["log"]);
    assert!(ok, "{stderr}");
    let entries = log["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{log}");
    assert_eq!(entries[0]["kind"], "revision");
    assert_eq!(entries[0]["change_id"], push["tip_change"]["change_id"]);
}

/// An amend (same Change-Id, new sha) appends a second revision (rev 1); a
/// re-push with nothing moved is idempotent and adds no entry.
#[test]
fn amend_appends_a_revision_idempotent_repush_does_not() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, _push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");

    let c1b = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nB\n")]);
    g.branch("feat", c1b);
    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");
    assert_eq!(push["tip_change"]["revision"], 1, "amend is rev 1: {push}");

    let (ok, log, stderr) = nit(&server, &g, &["log"]);
    assert!(ok, "{stderr}");
    let entries = log["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{log}");
    assert!(entries.iter().all(|e| e["kind"] == "revision"));

    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");
    assert_eq!(push["tip_change"]["revision"], 1);
    let (_ok, log, _) = nit(&server, &g, &["log"]);
    assert_eq!(log["entries"].as_array().unwrap().len(), 2, "{log}");
}

/// `nit comment --change-id` opens a thread; `--thread … --resolve` replies and
/// resolves it; `--change <numeric>` targets the same change by id.
#[test]
fn comment_opens_replies_resolves() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (ok, _push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");

    // Open a new thread on the change, resolved by the cwd's Change-Id. Returns
    // a Thread (review_id null → agent), born unresolved.
    let (ok, thread, stderr) = nit(
        &server,
        &g,
        &[
            "comment",
            "--change-id",
            "Ia",
            "--file",
            "a.txt",
            "--line",
            "1",
            "-m",
            "is this right?",
        ],
    );
    assert!(ok, "{stderr}");
    assert_eq!(thread["resolved"], false);
    assert!(thread["comments"][0]["review_id"].is_null());
    assert_eq!(thread["comments"][0]["body"], "is this right?");
    let thread_id = thread["id"].as_u64().unwrap();
    let change_num = thread["change_id"].as_u64().unwrap();

    let (ok, reply, stderr) = nit(
        &server,
        &g,
        &[
            "comment",
            "--change-id",
            "Ia",
            "--thread",
            &thread_id.to_string(),
            "-m",
            "fixed it",
            "--resolve",
        ],
    );
    assert!(ok, "{stderr}");
    assert_eq!(reply["resolved"], true);
    let comments = reply["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments.last().unwrap()["body"], "fixed it");

    // `--change <numeric id>` targets the same change as `--change-id Ia`.
    let (ok, opened, stderr) = nit(
        &server,
        &g,
        &[
            "comment",
            "--change",
            &change_num.to_string(),
            "-m",
            "by numeric id",
        ],
    );
    assert!(ok, "{stderr}");
    assert_eq!(opened["change_id"], change_num);
    assert_eq!(opened["comments"][0]["body"], "by numeric id");
}

/// `nit reopen` clears an abandoned change back to its retained status so a new
/// revision can be pushed. Abandonment is the background timer's call, so the
/// timer is sped up and the API polled until it lands.
#[test]
fn reopen_an_abandoned_change() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");
    let change_id = push["tip_change"]["change_id"].as_u64().unwrap();

    // CLI abandon — a reviewer/agent judgment, distinct from the background timer.
    let (ok, detail, stderr) = nit(
        &server,
        &g,
        &["abandon", "--change", &change_id.to_string()],
    );
    assert!(ok, "{stderr}");
    assert_eq!(detail["id"], change_id);

    // Clears the change back to non-terminal status.
    let (ok, detail, stderr) = nit(&server, &g, &["reopen", "--change", &change_id.to_string()]);
    assert!(ok, "{stderr}");
    assert_eq!(detail["id"], change_id);
    assert_eq!(detail["change_key"], "Ia");

    // No 409 gate after reopen — the change accepts a new push.
    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "reopened change accepts a push: {stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
}

/// Push fails when any commit lacks a `Change-Id` — the all-or-nothing walk rejects the branch.
#[test]
fn push_without_change_id_fails_with_a_helpful_message() {
    let g = GitRepo::new();
    // No Change-Id trailer on this commit.
    let c1 = g.commit(&[g.root], "core: add a\n", &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, _json, stderr) = nit_register(&server, &g, "feat");
    assert!(!ok, "a missing Change-Id must fail the push");
    assert!(
        stderr.contains("Change-Id trailer"),
        "the error names the missing trailer: {stderr}"
    );
}

/// `nit status` before any push fails non-zero, telling the agent to push first.
#[test]
fn status_before_any_push_says_run_nit_push_first() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let out = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["status"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &server.base)
        .output()
        .unwrap();
    assert!(!out.status.success(), "status before push must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("run 'nit push' first"), "{stderr}");
}

/// Unreachable server is reported as a connection error, not as a malformed-request error.
#[test]
fn push_to_a_dead_server_reports_unreachable() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();

    // Start then drop a server to claim (and free) a port the client will hit.
    let dead = TestServer::start(g.dir.path().join("dead.sqlite3"), None);
    let base = dead.base.clone();
    drop(dead);

    let out = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["push", "feat"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &base)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("is 'nit serve' running?"), "{stderr}");
}

/// Bare `nit push` (no args) resolves the cwd's checked-out commit — the agent
/// registers the repo, commits, and simply pushes.
#[test]
fn bare_push_resolves_head() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, _, stderr) = nit(&server, &g, &["repo", "create", "--base", "main"]);
    assert!(ok, "repo create: {stderr}");
    let (ok, push, stderr) = nit(&server, &g, &["push"]);
    assert!(ok, "bare push resolves HEAD: {stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
}

/// A detached HEAD has no branch name, yet bare `nit push` resolves the
/// checked-out commit just the same.
#[test]
fn push_resolves_detached_head() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.repo.set_head_detached(c1).unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, _, stderr) = nit(&server, &g, &["repo", "create", "--base", "main"]);
    assert!(ok, "repo create: {stderr}");
    let (ok, push, stderr) = nit(&server, &g, &["push"]);
    assert!(ok, "detached HEAD resolves: {stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
}
