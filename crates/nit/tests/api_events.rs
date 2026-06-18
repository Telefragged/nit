//! `WS /api/stream`: backlog replay, the idx watermark, live streaming, and the
//! `new_parent` advisory (docs/api.md "Events").

mod common;

use std::time::Duration;

use common::{GitRepo, TestServer, http_post, member_id, msg, push, ws_read, ws_subscribe};
use serde_json::json;

const READ: Duration = Duration::from_secs(3);

/// A `subscribe` from idx 0 replays the change's backlog, then live appends
/// stream in with monotone seq.
#[test]
fn subscribe_replays_backlog_then_streams_live() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&res, "I001");

    let mut socket = ws_subscribe(&server, &[(change_id, 0)], READ);
    let backlog = ws_read(&mut socket).expect("backlog revision entry");
    assert_eq!(backlog["change_id"], change_id);
    assert_eq!(backlog["idx"], 0);
    assert_eq!(backlog["kind"], "revision");

    // A live review entry streams in past the backlog.
    let (rst, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 0, "verdict": "request_changes", "message": "fix"}),
    );
    assert_eq!(rst, 200);
    let live = ws_read(&mut socket).expect("live review entry");
    assert_eq!(live["kind"], "review");
    assert_eq!(live["idx"], 1);
    assert!(
        live["seq"].as_u64().unwrap() > backlog["seq"].as_u64().unwrap(),
        "seq is monotone: {live} after {backlog}"
    );
}

/// A `subscribe` from the change's head replays nothing (the watermark/empty
/// backlog), then a live append arrives — the doorbell `nit wait` relies on.
#[test]
fn subscribe_at_head_skips_backlog() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main", None);
    let change_id = member_id(&res, "I001");

    // The revision is at idx 0, so head is idx 1: no backlog replays.
    let mut socket = ws_subscribe(&server, &[(change_id, 1)], Duration::from_millis(400));
    assert!(ws_read(&mut socket).is_none(), "no backlog at head");

    // Resubscribe is not needed; the live append on this socket arrives.
    let (rst, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 0, "verdict": "approve", "message": "lgtm"}),
    );
    assert_eq!(rst, 200);
    let live = ws_read(&mut socket).expect("live entry after head subscribe");
    assert_eq!(live["kind"], "review");
    assert_eq!(live["idx"], 1);
}

/// Only entries for currently-subscribed changes reach a socket.
#[test]
fn unsubscribed_changes_are_silent() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main", None);
    let one = member_id(&res, "I001");
    let two = member_id(&res, "I002");

    // Subscribe only to change one, at its head.
    let mut socket = ws_subscribe(&server, &[(one, 1)], Duration::from_millis(400));
    // Activity on change two must not reach this socket.
    let (rst, _) = http_post(
        &server.url(&format!("/api/changes/{two}/reviews")),
        &json!({"revision": 0, "verdict": "approve", "message": "ok"}),
    );
    assert_eq!(rst, 200);
    assert!(
        ws_read(&mut socket).is_none(),
        "change two is not subscribed"
    );
}
