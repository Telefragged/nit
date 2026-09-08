//! `nit log --wait` over the websocket: it drains the log, waking on any new
//! entry, and parks on the stream until fresh activity lands.

mod common;

use std::time::Duration;

use common::{
    GitRepo, TestServer, change_by_label, first_repo_id, get_changes, http_get, msg, nit,
    nit_register, nit_spawn, review,
};

/// Registers the repo (a push needs one to exist), bare-pushes the cwd HEAD,
/// and returns the number of the change the push registered.
fn push_head(server: &TestServer, g: &GitRepo) -> u64 {
    let (ok, _, err) = nit_register(server, g);
    assert!(ok, "push failed: {err}");
    get_changes(server, "")[0]["id"]
        .as_u64()
        .expect("the registered change")
}

/// The highest sequence the repo's log holds.
fn head_sequence(server: &TestServer) -> u64 {
    let repo_id = first_repo_id(server);
    let (_, log) = http_get(&server.url(&format!("/api/log?repo={repo_id}")));
    log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["sequence"].as_u64())
        .max()
        .unwrap()
}

/// Long enough for a spawned `--wait` to read the log and open its socket
/// before the test acts, so the test exercises the socket path.
const PARKED: Duration = Duration::from_millis(400);

/// `nit log --wait 0` wakes immediately on any existing activity past the cursor
/// (here, the author's own push revision), printing the digest and the entry.
#[test]
fn wait_returns_existing_activity() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    push_head(&server, &g);

    let (ok, out, err) =
        nit_spawn(&server, &g, &["log", "--wait", "0"]).finish(Duration::from_secs(15));
    assert!(ok, "wait failed: {err}");
    let out = out.as_str().expect("wait prints text");
    assert!(out.contains("cursor="), "prints the digest header: {out}");
    assert!(
        out.contains("revision"),
        "surfaced the revision entry: {out}"
    );
}

#[test]
fn wait_blocks_then_wakes_on_a_review() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let change_number = push_head(&server, &g);
    // The head sequence after the push: the author's own entries.
    let head_seq = head_sequence(&server);

    let wait = nit_spawn(&server, &g, &["log", "--wait", &head_seq.to_string()]);
    std::thread::sleep(PARKED);
    review(&server, change_number, "request_changes", "fix the unwrap");

    let (ok, out, err) = wait.finish(Duration::from_secs(20));
    assert!(ok, "wait failed: {err}");
    let out = out.as_str().expect("wait prints text");
    assert!(
        out.contains("reviewer: request_changes"),
        "woke on the review: {out}"
    );
    // The digest shows the tag the wait read by and the change's status
    // after the review.
    assert!(out.starts_with("cursor="), "{out}");
    assert!(out.contains("tag worktree="), "{out}");
    assert!(out.contains("changes_requested"), "{out}");
}

/// A `--wait` started on the branch returns on a review of a change pushed
/// to the branch after the wait started.
#[test]
fn wait_wakes_on_a_change_pushed_after_it_parked() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    push_head(&server, &g);
    let head_seq = head_sequence(&server);

    let wait = nit_spawn(
        &server,
        &g,
        &["log", "--wait", "--reviewer-only", &head_seq.to_string()],
    );
    std::thread::sleep(PARKED);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let (ok, _, err) = nit(&server, &g, &["push"]);
    assert!(ok, "push failed: {err}");
    let two = change_by_label(&server, first_repo_id(&server), "I002")["id"]
        .as_u64()
        .expect("the new change");
    review(&server, two, "request_changes", "fix the unwrap");

    let (ok, out, err) = wait.finish(Duration::from_secs(20));
    assert!(ok, "wait failed: {err}");
    let out = out.as_str().expect("wait prints text");
    assert!(
        out.contains("reviewer: request_changes"),
        "woke on the review of the new change: {out}"
    );
    assert!(
        out.contains("I002"),
        "the digest lists the new change: {out}"
    );
}
