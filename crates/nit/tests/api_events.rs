//! `WS /api/stream`: the projection watermark, the tag subscription, and
//! live streaming.

mod common;

use std::time::Duration;

use common::{
    GitRepo, TestServer, first_repo_id, member_id, msg, push, review, tag_change, ws_entry,
    ws_read, ws_subscribe_projection, ws_subscribe_tagged,
};
use serde_json::json;

const READ: Duration = Duration::from_secs(3);

/// Projection mode ships the folded `ChangeProjection` (its `entries_folded` the
/// high-water mark), then attaches the live tail past it.
#[test]
fn subscribe_projection_ships_it_then_streams_live() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&res, "I001");

    let mut socket = ws_subscribe_projection(&server, &[change_number], READ);
    let snap = ws_read(&mut socket).expect("projection frame")["projection"].clone();
    assert_eq!(snap["id"], change_number);
    assert_eq!(snap["revisions"].as_array().expect("revisions").len(), 1);
    // One entry (the revision) is folded, so the live tail resumes at position 1.
    assert_eq!(snap["entries_folded"], 1);

    review(&server, change_number, "approve", "lgtm");
    let live = ws_entry(&mut socket).expect("live review entry past the projection");
    assert_eq!(live["kind"], "review");
    assert_eq!(live["position"], 1);
}

/// A tag subscription first sends the tagged change's stored entries past
/// the cursor. Then it sends the change's new entries, and not another
/// change's. When a `tags` entry gives another change the tag, it sends
/// that entry and that change's later entries.
#[test]
fn subscribe_tagged_follows_the_changes_that_carry_the_tags() {
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

    // The stored entries: the tagged change's revision and tags entries. The
    // revision is included even though it was written before the tag.
    let mut socket = ws_subscribe_tagged(&server, repo_id, &json!({"branch": "feat"}), 0, READ);
    let revision = ws_entry(&mut socket).expect("backlog revision");
    assert_eq!(
        (
            revision["change_number"].as_u64(),
            revision["kind"].as_str()
        ),
        (Some(one), Some("revision"))
    );
    let tags = ws_entry(&mut socket).expect("backlog tags");
    assert_eq!(tags["kind"], "tags");

    // Same check as `unsubscribed_changes_are_silent`: the untagged change's
    // review is published first, so if the server sent it, it would arrive
    // before the tagged change's review.
    review(&server, two, "approve", "ok");
    review(&server, one, "request_changes", "fix");
    let live = ws_entry(&mut socket).expect("live review on the tagged change");
    assert_eq!(live["change_number"], one);
    assert_eq!(live["kind"], "review");

    let (st, res) = tag_change(&server, two, &json!({"branch": "feat"}));
    assert_eq!(st, 200, "{res}");
    let joined = ws_entry(&mut socket).expect("the tags entry that adds two");
    assert_eq!(
        (joined["change_number"].as_u64(), joined["kind"].as_str()),
        (Some(two), Some("tags"))
    );
    review(&server, two, "approve", "now followed");
    let live = ws_entry(&mut socket).expect("live review on the newly tagged change");
    assert_eq!(live["change_number"], two);
    assert_eq!(live["kind"], "review");

    // `after` is exclusive, so a subscription at the last sequence gets no
    // stored entries.
    let head = live["sequence"].as_u64().expect("sequence");
    let mut parked = ws_subscribe_tagged(
        &server,
        repo_id,
        &json!({"branch": "feat"}),
        head,
        Duration::from_millis(400),
    );
    assert!(ws_read(&mut parked).is_none(), "no backlog at head");
}

#[test]
fn unsubscribed_changes_are_silent() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (_, res) = push(&server, &g, "feat", "main");
    let one = member_id(&res, "I001");
    let two = member_id(&res, "I002");

    // Subscribe only to change one; reading its projection is the sync
    // point that puts the subscription in place before any review broadcasts.
    let mut socket = ws_subscribe_projection(&server, &[one], READ);
    assert!(ws_read(&mut socket).is_some_and(|f| f["projection"].is_object()));

    // Review change two (unsubscribed) then change one (subscribed). The next
    // frame must be change one's review: two's review is broadcast first, so a
    // leak would arrive ahead of one's. A deterministic silence fence, with no
    // read-timeout wait.
    review(&server, two, "approve", "ok");
    review(&server, one, "approve", "ok");
    let frame = ws_entry(&mut socket).expect("review for one");
    assert_eq!(frame["change_number"], one);
    assert_eq!(frame["kind"], "review");
    assert_eq!(frame["position"], 1);
}
