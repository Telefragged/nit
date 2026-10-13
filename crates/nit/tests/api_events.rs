//! `WS /api/stream`: projections, entries past the positions, and `left`.

mod common;

use common::{
    GitRepo, TestServer, first_repo_id, member_id, msg, push, review, tag_change, ws_entry,
    ws_read, ws_subscribe,
};
use serde_json::{Value, json};

/// The next frame's `projection`, which must be the next frame.
fn ws_projection(socket: &mut common::WsSock) -> Value {
    let frame = ws_read(socket);
    assert!(frame["projection"].is_object(), "not a projection: {frame}");
    frame["projection"].clone()
}

/// Publishes a review on `change` and returns the entry the socket reads.
///
/// The server sends the messages of a read before the messages of a later
/// commit. So a message that the socket should not have got arrives ahead
/// of this one.
fn fence(server: &TestServer, socket: &mut common::WsSock, change: u64) -> Value {
    review(server, change, "comment", "fence");
    let entry = ws_entry(socket);
    assert_eq!(entry["change_number"].as_u64(), Some(change));
    assert_eq!(entry["kind"], "review");
    entry
}

/// A subscription without positions sends the projection of the matched
/// change, then its new entries. A change number alone matches the
/// change, whatever its repo.
#[test]
fn subscribe_ships_the_projection_then_streams_live() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&res, "I001");

    let query = json!({ "change": change_number });
    let mut socket = ws_subscribe(&server, &json!({ "query": query }));
    let snap = ws_projection(&mut socket);
    assert_eq!(snap["id"], change_number);
    assert_eq!(snap["revisions"].as_array().expect("revisions").len(), 1);
    assert_eq!(snap["entries_folded"], 1);

    review(&server, change_number, "approve", "lgtm");
    let live = ws_entry(&mut socket);
    assert_eq!(live["kind"], "review");
    assert_eq!(live["position"], 1);
}

/// A tag subscription follows the set of changes that carry the tag. A
/// change that gets the tag arrives as its projection, and a change that
/// loses the tag arrives as `left`.
#[test]
fn subscribe_by_tag_follows_the_changes_that_carry_the_tag() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", a);
    let b = g.commit(&[g.root], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("other", b);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main");
    let one = member_id(&res, "I001");
    let (_, res) = push(&server, &g, "other", "main");
    let two = member_id(&res, "I002");
    let (st, res) = tag_change(&server, one, &json!({"branch": "feat"}));
    assert_eq!(st, 200, "{res}");
    let repo_id = first_repo_id(&server);
    let query = json!({ "repo": repo_id, "tag": ["branch=feat"] });

    let mut socket = ws_subscribe(&server, &json!({ "query": query }));
    assert_eq!(ws_projection(&mut socket)["id"], one);

    // The review of the untagged change commits first. So if the server
    // sent it, it would arrive before the review of the tagged change.
    review(&server, two, "approve", "ok");
    fence(&server, &mut socket, one);

    let (st, res) = tag_change(&server, two, &json!({"branch": "feat"}));
    assert_eq!(st, 200, "{res}");
    let met = ws_projection(&mut socket);
    assert_eq!(met["id"], two);
    assert_eq!(
        met["entries_folded"], 3,
        "the projection holds the tags entry"
    );
    fence(&server, &mut socket, two);

    let (st, res) = tag_change(&server, one, &json!({"branch": "elsewhere"}));
    assert_eq!(st, 200, "{res}");
    assert_eq!(ws_read(&mut socket), json!({ "left": one }));
}

/// A subscription with positions sends the entries of each held change
/// past its position, and `left` for a held change outside the query.
#[test]
fn subscribe_with_positions_sends_the_entries_past_them() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main");
    let one = member_id(&res, "I001");
    let two = member_id(&res, "I002");
    review(&server, one, "approve", "ok");
    review(&server, one, "comment", "again");

    let positions = json!({ one.to_string(): 2, two.to_string(): 0 });
    let mut socket = ws_subscribe(
        &server,
        &json!({ "query": { "change": one }, "positions": positions }),
    );
    let stored = ws_entry(&mut socket);
    assert_eq!(
        (
            stored["change_number"].as_u64(),
            stored["position"].as_u64()
        ),
        (Some(one), Some(2))
    );
    assert_eq!(ws_read(&mut socket), json!({ "left": two }));
    let live = fence(&server, &mut socket, one);
    assert_eq!(live["position"], 3);
}

/// A `log` start sends every entry of a change that the positions lack,
/// in place of its projection.
#[test]
fn a_log_start_sends_every_entry() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main");
    let one = member_id(&res, "I001");
    review(&server, one, "approve", "ok");

    let mut socket = ws_subscribe(
        &server,
        &json!({ "query": { "change": one }, "start": "log" }),
    );
    assert_eq!(ws_entry(&mut socket)["kind"], "revision");
    assert_eq!(ws_entry(&mut socket)["kind"], "review");
    let live = fence(&server, &mut socket, one);
    assert_eq!(live["position"], 2);
}
