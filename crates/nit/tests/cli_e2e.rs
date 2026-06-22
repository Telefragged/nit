//! End-to-end CLI: the real `nit` binary (`CARGO_BIN_EXE`) run from inside a
//! fixture repo against a real server. The agent drives push / ready / status /
//! log / comment / reopen one-shot (the live followers `nit wait` /
//! `nit log --follow` return in a later stage) — docs/agent-workflow.md.
//!
//! `nit push` walks the change-centric model: oldest-first, upsert each change
//! by its `Change-Id`, append a revision iff the sha moved (revisions are
//! 0-based). `nit status`/`nit log` resolve the cwd's tip change from local
//! HEAD, then read the derived chain on demand.

mod common;

use std::process::Command;

use common::{GitRepo, TestServer, msg, nit, nit_register, review};

/// `nit push` prints a `PushResult` (`tip_change` + the derived chain) and
/// registers the chain; `nit status`/`nit log` then read it back, resolved from
/// the cwd HEAD.
#[test]
fn push_prints_result_then_status_and_log_read_it_back() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap(); // the agent's checkout
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // push <branch> from the cwd, base auto-detected (main). The result carries
    // the tip change at rev 0 (0-based) and the tip-rooted chain.
    let (ok, push, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
    assert_eq!(push["tip_change"]["revision"], 0, "{push}");
    assert_eq!(push["tip_change"]["status"], "pending");
    let chain = &push["chain"];
    assert_eq!(chain["base_branch"], "main");
    assert_eq!(chain["state"], "waiting_for_review");
    assert_eq!(chain["partial"], false);
    assert_eq!(chain["path"].as_array().unwrap().len(), 1);
    assert_eq!(chain["path"][0]["position"], 0);
    assert_eq!(chain["path"][0]["change_key"], "Ia");
    assert_eq!(chain["path"][0]["revision"], 0);

    // status (no --oneline) reads `GET /api/chains/{tip}` for the cwd HEAD.
    let (ok, status, stderr) = nit(&server, &g, &["status"]);
    assert!(ok, "{stderr}");
    assert_eq!(status["state"], "waiting_for_review");
    assert_eq!(status["path"][0]["change_key"], "Ia");
    assert_eq!(status["path"][0]["revision"], 0);

    // status --oneline: a compact state line plus one line per member.
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

    // log: the aggregated chain log, one revision entry from the push.
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

    let (ok, _push, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");

    // amend (same Change-Id Ia, new content -> new sha) = rev 1.
    let c1b = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nB\n")]);
    g.branch("feat", c1b);
    let (ok, push, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    assert_eq!(push["tip_change"]["revision"], 1, "amend is rev 1: {push}");

    // The aggregated log now has two revision entries.
    let (ok, log, stderr) = nit(&server, &g, &["log"]);
    assert!(ok, "{stderr}");
    let entries = log["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{log}");
    assert!(entries.iter().all(|e| e["kind"] == "revision"));

    // Re-push with nothing moved: idempotent, still rev 1, no new entry.
    let (ok, push, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    assert_eq!(push["tip_change"]["revision"], 1);
    let (_ok, log, _) = nit(&server, &g, &["log"]);
    assert_eq!(log["entries"].as_array().unwrap().len(), 2, "{log}");
}

/// `nit push --partial` marks the tip partial; `nit ready` clears it. While
/// partial, an all-approved chain is `agents_turn`, never `approved`.
#[test]
fn partial_push_then_ready_clears_it() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, push, stderr) = nit_register(&server, &g, "push", "feat", &["--partial"]);
    assert!(ok, "{stderr}");
    assert_eq!(push["chain"]["partial"], true);
    assert_eq!(push["chain"]["state"], "waiting_for_review");
    let change_id = push["tip_change"]["change_id"].as_u64().unwrap();

    // Reviewer approves the only change (stage a decision + submit the chain,
    // as the browser would). The verdict lands on rev 0.
    review(&server, change_id, "approve", "lgtm");

    // Approved but still partial: agents_turn, never approved.
    let (ok, status, stderr) = nit(&server, &g, &["status"]);
    assert!(ok, "{stderr}");
    assert_eq!(status["state"], "agents_turn");
    assert_eq!(status["partial"], true);
    assert_eq!(status["path"][0]["status"], "approved");

    // ready clears the sticky flag; the approved chain becomes mergeable.
    let (ok, ready, stderr) = nit_register(&server, &g, "ready", "feat", &[]);
    assert!(ok, "{stderr}");
    assert_eq!(ready["chain"]["partial"], false);
    assert_eq!(ready["chain"]["state"], "approved");
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
    let (ok, _push, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");

    // Open a new thread on the change, resolved by the cwd's Change-Id. Returns
    // a Thread (author=agent), born unresolved.
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
    assert_eq!(thread["comments"][0]["author"], "agent");
    assert_eq!(thread["comments"][0]["body"], "is this right?");
    let thread_id = thread["id"].as_u64().unwrap();
    let change_num = thread["change_id"].as_u64().unwrap();

    // Reply to the thread and resolve it in one shot (--thread … --resolve).
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

    let (ok, push, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "{stderr}");
    let change_id = push["tip_change"]["change_id"].as_u64().unwrap();

    // Abandon it explicitly via the CLI (a reviewer/agent judgment).
    let (ok, detail, stderr) = nit(
        &server,
        &g,
        &["abandon", "--change", &change_id.to_string()],
    );
    assert!(ok, "{stderr}");
    assert_eq!(detail["id"], change_id);

    // reopen by id clears it back to non-terminal.
    let (ok, detail, stderr) = nit(&server, &g, &["reopen", "--change", &change_id.to_string()]);
    assert!(ok, "{stderr}");
    assert_eq!(detail["id"], change_id);
    assert_eq!(detail["change_key"], "Ia");

    // After reopen the change is pushable again (no 409 gate).
    let (ok, push, stderr) = nit_register(&server, &g, "push", "feat", &[]);
    assert!(ok, "reopened change accepts a push: {stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
}

/// A push from a branch with a commit missing its `Change-Id` trailer fails
/// non-zero with a helpful message (the all-or-nothing walk rejects it).
#[test]
fn push_without_change_id_fails_with_a_helpful_message() {
    let g = GitRepo::new();
    // No Change-Id trailer on this commit.
    let c1 = g.commit(&[g.root], "core: add a\n", &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, _json, stderr) = nit_register(&server, &g, "push", "feat", &[]);
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

/// An unreachable server is reported as such, not as a malformed request.
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

    let (ok, _, stderr) = nit(&server, &g, &["repo", "create"]);
    assert!(ok, "repo create: {stderr}");
    let (ok, push, stderr) = nit(&server, &g, &["push"]);
    assert!(ok, "bare push resolves HEAD: {stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
    assert_eq!(push["chain"]["base_branch"], "main");
}

/// A detached HEAD has no branch name, yet bare `nit push` resolves the
/// checked-out commit just the same.
#[test]
fn push_resolves_detached_head() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.repo.set_head_detached(c1).unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, _, stderr) = nit(&server, &g, &["repo", "create"]);
    assert!(ok, "repo create: {stderr}");
    let (ok, push, stderr) = nit(&server, &g, &["push"]);
    assert!(ok, "detached HEAD resolves: {stderr}");
    assert_eq!(push["tip_change"]["change_key"], "Ia");
}
