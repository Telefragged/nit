//! `/wait` long-poll semantics over real HTTP: cursor bootstrap, blocking
//! until an event lands, timeout behavior (docs/api.md).

mod common;

use std::time::{Duration, Instant};

use common::{GitRepo, TestServer, http_get, http_post, msg};
use serde_json::json;

fn setup() -> (GitRepo, TestServer, i64) {
    let g = GitRepo::new();
    let c1 = g.commit(
        &[g.root],
        &msg("core: thing", "Ia"),
        &[("a.txt", "one\ntwo\n")],
    );
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, chain) = http_post(
        &server.url("/api/chains"),
        &json!({
            "repo_path": g.workdir().to_string_lossy(),
            "branch": "feat",
            "base": "main",
        }),
    );
    assert_eq!(st, 200, "{chain}");
    let chain_id = chain["id"].as_i64().unwrap();
    (g, server, chain_id)
}

#[test]
fn wait_bootstrap_and_timeout() {
    let (_g, server, chain_id) = setup();

    // cursor=0 returns the current snapshot immediately.
    let started = Instant::now();
    let (st, boot) = http_get(&server.url(&format!("/api/chains/{chain_id}/wait?cursor=0")));
    assert_eq!(st, 200);
    assert!(started.elapsed() < Duration::from_secs(5));
    let cursor = boot["cursor"].as_i64().unwrap();
    assert!(cursor > 0);
    assert_eq!(boot["feedback"]["state"], "waiting_for_review");

    // An up-to-date cursor blocks until the timeout, then returns the
    // unchanged snapshot.
    let started = Instant::now();
    let (st, idle) = http_get(&server.url(&format!(
        "/api/chains/{chain_id}/wait?cursor={cursor}&timeout=1"
    )));
    assert_eq!(st, 200);
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(900), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    assert_eq!(idle["cursor"].as_i64().unwrap(), cursor);
    assert_eq!(idle["feedback"]["actionable"], false);

    // Unknown chain → 404.
    let (st, _) = http_get(&server.url("/api/chains/999/wait?cursor=0"));
    assert_eq!(st, 404);
}

#[test]
fn wait_wakes_up_when_the_reviewer_acts() {
    let (_g, server, chain_id) = setup();
    let (_, chain) = http_get(&server.url(&format!("/api/chains/{chain_id}")));
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    let (_, boot) = http_get(&server.url(&format!("/api/chains/{chain_id}/wait?cursor=0")));
    let cursor = boot["cursor"].as_i64().unwrap();

    let wait_url = server.url(&format!(
        "/api/chains/{chain_id}/wait?cursor={cursor}&timeout=30"
    ));
    let started = Instant::now();
    let waiter = std::thread::spawn(move || http_get(&wait_url));

    // Let the poll actually block before acting.
    std::thread::sleep(Duration::from_millis(500));
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "comment", "message": "question?"}),
    );
    assert_eq!(st, 200);

    let (st, woke) = waiter.join().unwrap();
    let elapsed = started.elapsed();
    assert_eq!(st, 200);
    assert!(
        elapsed >= Duration::from_millis(450),
        "returned before the event existed: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "hit the timeout instead of waking: {elapsed:?}"
    );
    assert!(woke["cursor"].as_i64().unwrap() > cursor);
    assert_eq!(woke["feedback"]["state"], "agents_turn");
    assert_eq!(woke["feedback"]["changes"][0]["status"], "commented");
}
