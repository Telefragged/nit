//! `WS /api/stream`: backlog replay, the idx watermark, live streaming, and the
//! `new_parent` advisory (docs/api.md "Events").

mod common;

use std::time::Duration;

use common::{GitRepo, TestServer, member_id, msg, push, review, ws_read, ws_subscribe};

const READ: Duration = Duration::from_secs(3);

/// A `subscribe` from idx 0 replays the change's backlog, then live appends
/// stream in with monotone seq.
#[test]
fn subscribe_replays_backlog_then_streams_live() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&server, &res, "I001");

    let mut socket = ws_subscribe(&server, &[(change_id, 0)], READ);
    let backlog = ws_read(&mut socket).expect("backlog revision entry");
    assert_eq!(backlog["change_id"], change_id);
    assert_eq!(backlog["idx"], 0);
    assert_eq!(backlog["kind"], "revision");

    // A live review entry streams in past the backlog. review() stages a
    // decision (a side-table write, no log entry/frame) then submits, which
    // appends the `review` at idx 1 and broadcasts it.
    review(&server, change_id, "request_changes", "fix");
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
    let (_, res) = push(&server, &g, "feat", "main");
    let change_id = member_id(&server, &res, "I001");

    // The revision is at idx 0, so head is idx 1: no backlog replays.
    let mut socket = ws_subscribe(&server, &[(change_id, 1)], Duration::from_millis(400));
    assert!(ws_read(&mut socket).is_none(), "no backlog at head");

    // Resubscribe is not needed; the live append on this socket arrives.
    review(&server, change_id, "approve", "lgtm");
    let live = ws_read(&mut socket).expect("live entry after head subscribe");
    assert_eq!(live["kind"], "review");
    assert_eq!(live["idx"], 1);
}

/// A brand-new tip stacked on a subscribed change wakes that change's
/// follower with a `new_parent` advisory — the chain-extension counterpart to
/// a re-root. Without it, `nit log --follow` on an earlier change never learns
/// the new tip exists and misses reviews published to it. The advisory lands on
/// the *parent's* feed; the new child's own feed has no subscribers yet.
#[test]
fn stacked_tip_wakes_parent_follower() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main");
    let one = member_id(&server, &res, "I001");

    // Follow change one from idx 0; reading its backlog revision arms the feed
    // (the sync point) before we stack the tip.
    let mut socket = ws_subscribe(&server, &[(one, 0)], READ);
    let backlog = ws_read(&mut socket).expect("backlog revision for change one");
    assert_eq!(backlog["change_id"], one);
    assert_eq!(backlog["kind"], "revision");

    // Stack a brand-new tip (change two) on change one and re-push.
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let (st, res2) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res2}");
    let two = member_id(&server, &res2, "I002");

    // change one's follower is woken by the advisory naming the new child —
    // change one itself appended nothing (its content is unchanged).
    let frame = ws_read(&mut socket).expect("new_parent advisory on the parent feed");
    assert_eq!(frame["new_parent"]["of"], two);
    assert_eq!(frame["new_parent"]["parent"], one);
}

/// Only entries for currently-subscribed changes reach a socket.
#[test]
fn unsubscribed_changes_are_silent() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main");
    let one = member_id(&server, &res, "I001");
    let two = member_id(&server, &res, "I002");

    // Subscribe only to change one, at its head.
    let mut socket = ws_subscribe(&server, &[(one, 1)], Duration::from_millis(400));
    // Activity on change two must not reach this socket.
    review(&server, two, "approve", "ok");
    assert!(
        ws_read(&mut socket).is_none(),
        "change two is not subscribed"
    );
}
