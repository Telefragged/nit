//! `WS /api/stream`: the subscription's opening frames and live streaming.

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
/// The socket sends its stored entries before any entry published after
/// it opened, so a stored entry the subscription should not have got
/// would arrive ahead of this one.
fn fence(server: &TestServer, socket: &mut common::WsSock, change: u64) -> Value {
    review(server, change, "comment", "fence");
    let entry = ws_entry(socket);
    assert_eq!(entry["change_number"].as_u64(), Some(change));
    assert_eq!(entry["kind"], "review");
    entry
}

/// Without a cursor, a subscription ships the picked change's projection
/// (its `entries_folded` the high-water mark), then only its live entries.
/// A change number alone picks the change, whatever its repo.
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
    let mut socket = ws_subscribe(&server, &query, None);
    let snap = ws_projection(&mut socket);
    assert_eq!(snap["id"], change_number);
    assert_eq!(snap["revisions"].as_array().expect("revisions").len(), 1);
    // One entry (the revision) is folded, so the live tail resumes at position 1.
    assert_eq!(snap["entries_folded"], 1);

    review(&server, change_number, "approve", "lgtm");
    let live = ws_entry(&mut socket);
    assert_eq!(live["kind"], "review");
    assert_eq!(live["position"], 1);
}

/// A tag subscription first sends the tagged change's projection and, with
/// a cursor, its stored entries past the cursor. Then it sends the change's
/// new entries, and not another change's. When a `tags` entry gives
/// another change the tag, it sends that change's projection, the entry,
/// and that change's later entries.
#[test]
fn subscribe_by_tag_follows_the_changes_that_carry_the_tags() {
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

    // The stored entries: the tagged change's revision and tags entries,
    // behind its projection. The revision is included even though it was
    // written before the tag.
    let mut socket = ws_subscribe(&server, &query, Some(0));
    assert_eq!(ws_projection(&mut socket)["id"], one);
    let revision = ws_entry(&mut socket);
    assert_eq!(
        (
            revision["change_number"].as_u64(),
            revision["kind"].as_str()
        ),
        (Some(one), Some("revision"))
    );
    let tags = ws_entry(&mut socket);
    assert_eq!(tags["kind"], "tags");

    // Same check as `unpicked_changes_are_silent`: the untagged change's
    // review is published first, so if the server sent it, it would arrive
    // before the tagged change's review.
    review(&server, two, "approve", "ok");
    review(&server, one, "request_changes", "fix");
    let live = ws_entry(&mut socket);
    assert_eq!(live["change_number"], one);
    assert_eq!(live["kind"], "review");

    let (st, res) = tag_change(&server, two, &json!({"branch": "feat"}));
    assert_eq!(st, 200, "{res}");
    let met = ws_projection(&mut socket);
    assert_eq!(met["id"], two, "the socket meets two at its tags entry");
    assert_eq!(met["entries_folded"], 3, "the projection already holds it");
    let joined = ws_entry(&mut socket);
    assert_eq!(
        (joined["change_number"].as_u64(), joined["kind"].as_str()),
        (Some(two), Some("tags"))
    );
    review(&server, two, "approve", "now followed");
    let live = ws_entry(&mut socket);
    assert_eq!(live["change_number"], two);
    assert_eq!(live["kind"], "review");

    // Without a cursor, the subscription announces both picked changes and
    // sends no stored entry.
    let mut fresh = ws_subscribe(&server, &query, None);
    assert_eq!(ws_projection(&mut fresh)["id"], one);
    assert_eq!(ws_projection(&mut fresh)["id"], two);
    let fenced = fence(&server, &mut fresh, one);

    // `after` is exclusive, so a subscription at the last sequence gets the
    // projections and no stored entry.
    let head = fenced["sequence"].as_u64().expect("sequence");
    let mut parked = ws_subscribe(&server, &query, Some(head));
    assert_eq!(ws_projection(&mut parked)["id"], one);
    assert_eq!(ws_projection(&mut parked)["id"], two);
    fence(&server, &mut parked, two);
}

#[test]
fn unpicked_changes_are_silent() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main");
    let one = member_id(&res, "I001");
    let two = member_id(&res, "I002");
    let repo_id = first_repo_id(&server);

    // Subscribe only to change one; reading its projection is the sync
    // point that puts the subscription in place before any review broadcasts.
    let query = json!({ "repo": repo_id, "change_id": common::change_id("I001") });
    let mut socket = ws_subscribe(&server, &query, None);
    assert_eq!(ws_projection(&mut socket)["id"], one);

    // Review change two (unpicked) then change one (picked). The next frame
    // must be change one's review: two's review is broadcast first, so a
    // leak would arrive ahead of one's. A deterministic silence fence, with no
    // read-timeout wait.
    review(&server, two, "approve", "ok");
    review(&server, one, "approve", "ok");
    let frame = ws_entry(&mut socket);
    assert_eq!(frame["change_number"], one);
    assert_eq!(frame["kind"], "review");
    assert_eq!(frame["position"], 1);
}
