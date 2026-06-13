//! `/events` SSE stream: backlog replay from the cursor and live streaming
//! of every appended entry. The server emits the raw log with **no**
//! wake/relevance filtering — deciding which events matter is the client's
//! job (docs/api.md "events", docs/data-model.md "Wake rule").

mod common;

use std::time::Duration;

use common::{GitRepo, TestServer, http_get, http_post, msg, sse_collect};
use serde_json::json;

fn setup() -> (GitRepo, TestServer, i64, i64) {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: thing", "Ia"), &[("a.txt", "one\n")]);
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
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();
    (g, server, chain_id, change_id)
}

#[test]
fn events_replay_backlog_then_stream_every_entry_unfiltered() {
    let (_g, server, chain_id, change_id) = setup();

    // Connecting at cursor 0 replays the missed backlog immediately: here
    // the single `revisions` entry the push appended.
    let evs = sse_collect(
        &server.url(&format!("/api/chains/{chain_id}/events?cursor=0")),
        1,
        Duration::from_secs(3),
    );
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0]["idx"], 0);
    assert_eq!(evs[0]["kind"], "revisions");

    // A pure approve (no comments) is the one event the *client* suppresses,
    // yet the server still emits it on the stream — no server-side filtering.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": "lgtm"}),
    );
    assert_eq!(st, 200);
    let evs = sse_collect(
        &server.url(&format!("/api/chains/{chain_id}/events?cursor=1")),
        1,
        Duration::from_secs(3),
    );
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0]["idx"], 1);
    assert_eq!(evs[0]["kind"], "review");
    assert_eq!(evs[0]["payload"]["verdict"], "approve");
}

#[test]
fn events_stream_a_live_append() {
    let (_g, server, chain_id, change_id) = setup();

    // Park a reader caught up at head (cursor 1) in a thread, then act; the
    // appended review must arrive as a live event, not after a timeout.
    let url = server.url(&format!("/api/chains/{chain_id}/events?cursor=1"));
    let reader = std::thread::spawn(move || sse_collect(&url, 1, Duration::from_secs(5)));
    std::thread::sleep(Duration::from_millis(400));
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "fix"}),
    );
    assert_eq!(st, 200);

    let evs = reader.join().unwrap();
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0]["kind"], "review");
    assert_eq!(evs[0]["payload"]["verdict"], "request_changes");
}

#[test]
fn events_unknown_chain_is_404() {
    let (_g, server, _chain_id, _change_id) = setup();
    // The 404 is returned before any stream, so a plain GET sees it.
    let (st, _) = http_get(&server.url("/api/chains/999/events?cursor=0"));
    assert_eq!(st, 404);
}
