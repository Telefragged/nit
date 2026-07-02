//! End-to-end CLI: the real `nit` binary (`CARGO_BIN_EXE`) run from inside a
//! fixture repo against a real server. The agent drives push / status /
//! log / comment / reopen one-shot (the live followers `nit log --follow` /
//! `--wait` live in `cli_wait.rs`) — docs/agent-workflow.md.
//!
//! `nit push` walks the change-centric model: oldest-first, upsert each change
//! by its `Change-Id`, append a revision iff the sha moved (revisions are
//! 0-based). `nit status`/`nit log` resolve the cwd's tip change from local
//! HEAD, then read the derived chain on demand.

mod common;

use std::io::Write;
use std::process::Command;

use common::{GitRepo, TestServer, msg, nit, nit_register};

/// `nit push` prints the resulting chain digest and registers the chain;
/// `nit status`/`nit log` then read the derived chain back, resolved from the
/// cwd HEAD.
#[test]
fn push_prints_digest_then_status_and_log_read_it_back() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap(); // the agent's checkout
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");
    // push prints the chain digest — a `state=` header and one member line
    // (position change_key status rN Nu subject) — so no follow-up read.
    let push = push.as_str().expect("push prints text");
    assert!(push.contains("state=waiting_for_review"), "{push}");
    assert!(
        push.contains("Ia") && push.contains("pending") && push.contains("r0"),
        "{push}"
    );

    // status reads the derived chain back from the cwd HEAD and prints the same
    // digest.
    let (ok, status, stderr) = nit(&server, &g, &["status"]);
    assert!(ok, "{stderr}");
    let status = status.as_str().expect("status prints text");
    assert!(status.contains("state=waiting_for_review"), "{status}");
    assert!(status.contains("Ia") && status.contains("r0"), "{status}");

    let (ok, log, stderr) = nit(&server, &g, &["log"]);
    assert!(ok, "{stderr}");
    let log = log.as_str().expect("log prints text");
    assert!(
        log.contains("revision "),
        "one revision entry rendered: {log}"
    );
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
    assert!(
        push.as_str().is_some_and(|d| d.contains("r1")),
        "amend is rev 1: {push}"
    );

    let (ok, log, stderr) = nit(&server, &g, &["log"]);
    assert!(ok, "{stderr}");
    let log = log.as_str().expect("log prints text");
    assert_eq!(
        log.matches("revision ").count(),
        2,
        "two revision entries: {log}"
    );

    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");
    assert!(push.as_str().is_some_and(|d| d.contains("r1")), "{push}");
    let (_ok, log, _) = nit(&server, &g, &["log"]);
    assert_eq!(
        log.as_str().unwrap().matches("revision ").count(),
        2,
        "{log}"
    );
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

    // Open a new thread on the change, resolved by the cwd's Change-Id. The
    // confirmation names the thread, its anchor, and its state.
    let (ok, opened, stderr) = nit(
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
    let opened = opened.as_str().expect("comment prints text");
    assert!(opened.contains("opened thread"), "{opened}");
    assert!(opened.contains("a.txt:1"), "anchor shown: {opened}");
    assert!(
        opened.trim_end().ends_with("open"),
        "born unresolved: {opened}"
    );
    let thread_id = field_after(opened, "thread ");
    let change_num = field_after(opened, "on change ");

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
    let reply = reply.as_str().expect("comment prints text");
    assert!(
        reply.contains(&format!("replied on thread {thread_id}")),
        "{reply}"
    );
    assert!(reply.trim_end().ends_with("resolved"), "{reply}");

    // `--change <numeric id>` targets the same change as `--change-id Ia`.
    let (ok, by_num, stderr) = nit(
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
    let by_num = by_num.as_str().expect("comment prints text");
    assert!(
        by_num.contains(&format!("on change {change_num}")),
        "{by_num}"
    );
}

/// `-F` reads the body from a file; `-F -` reads stdin. `nit log` shows
/// both bodies back.
#[test]
fn comment_body_from_file_and_stdin() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (ok, _push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");

    let body_path = g.dir.path().join("body.md");
    std::fs::write(&body_path, "a body from a **file**\n").unwrap();
    let (ok, opened, stderr) = nit(
        &server,
        &g,
        &[
            "comment",
            "--change-id",
            "Ia",
            "-F",
            body_path.to_str().unwrap(),
        ],
    );
    assert!(ok, "{stderr}");
    assert!(
        opened.as_str().is_some_and(|o| o.contains("opened thread")),
        "{opened}"
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["comment", "--change-id", "Ia", "-F", "-"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &server.base)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("running nit");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"a body from stdin\n")
        .unwrap();
    let out = child.wait_with_output().expect("nit exits");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (ok, log, stderr) = nit(&server, &g, &["log"]);
    assert!(ok, "{stderr}");
    let log = log.as_str().expect("log prints text");
    assert!(log.contains("a body from a **file**"), "{log}");
    assert!(log.contains("a body from stdin"), "{log}");
}

/// The number following `marker` in a confirmation line (e.g. the id in
/// `opened thread 5 …` via `"thread "`).
fn field_after(text: &str, marker: &str) -> u64 {
    text.split(marker)
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|tok| tok.parse().ok())
        .unwrap_or_else(|| panic!("no number after {marker:?} in {text:?}"))
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

    let (ok, _push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "{stderr}");

    // CLI abandon — a reviewer/agent judgment, distinct from the background
    // timer — targeted by the cwd's Change-Id.
    let (ok, detail, stderr) = nit(&server, &g, &["abandon", "--change-id", "Ia"]);
    assert!(ok, "{stderr}");
    assert!(
        detail
            .as_str()
            .is_some_and(|d| d.contains("Ia") && d.contains("abandoned")),
        "{detail}"
    );

    let (ok, detail, stderr) = nit(&server, &g, &["reopen", "--change-id", "Ia"]);
    assert!(ok, "{stderr}");
    assert!(
        detail
            .as_str()
            .is_some_and(|d| d.contains("Ia") && d.contains("reopened")),
        "{detail}"
    );

    // No 409 gate after reopen — the change accepts a new push.
    let (ok, push, stderr) = nit_register(&server, &g, "feat");
    assert!(ok, "reopened change accepts a push: {stderr}");
    assert!(push.as_str().is_some_and(|d| d.contains("Ia")), "{push}");
}

/// Push fails when any commit lacks a `Change-Id` — the all-or-nothing walk rejects the branch.
#[test]
fn push_without_change_id_fails_with_a_helpful_message() {
    let g = GitRepo::new();
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

/// Bare `nit push` (no args) resolves the cwd's checked-out commit.
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
    assert!(push.as_str().is_some_and(|d| d.contains("Ia")), "{push}");
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
    assert!(push.as_str().is_some_and(|d| d.contains("Ia")), "{push}");
}
