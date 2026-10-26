//! The collection of closed changes (`nit::api::collect_changes`).

mod common;

use common::{
    GitRepo, TestServer, a_change, abandon, collect, http_get, msg, sweep, ws_read, ws_subscribe,
};
use jiff::{SignedDuration, Timestamp};
use nit::gitscan::objects::keep_ref_prefix;
use nit_types::domain::ChangeNumber;
use serde_json::json;

fn keep_refs(g: &GitRepo, change_number: u64) -> usize {
    g.repo
        .references_glob(&format!(
            "{}*",
            keep_ref_prefix(ChangeNumber::new(change_number))
        ))
        .expect("glob")
        .count()
}

/// A collection deletes the merged and abandoned changes and their keep
/// refs. It keeps the open changes.
#[test]
fn collect_deletes_closed_changes_and_their_keep_refs() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("one", a);
    let b = g.commit(&[g.root], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("two", b);
    let c = g.commit(&[g.root], &msg("three", "I003"), &[("c.txt", "c\n")]);
    g.branch("three", c);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let merged = a_change(&server, &g, "one");
    let abandoned = a_change(&server, &g, "two");
    let open = a_change(&server, &g, "three");
    g.branch("main", a);
    sweep(&server);
    abandon(&server, abandoned);
    assert_eq!(keep_refs(&g, abandoned), 1);

    let deleted = collect(&server, Timestamp::now() + SignedDuration::from_hours(1));

    assert_eq!(deleted, 2);
    for gone in [merged, abandoned] {
        let (st, _) = http_get(&server.url(&format!("/api/changes/{gone}")));
        assert_eq!(st, 404, "change {gone} is deleted");
        assert_eq!(keep_refs(&g, gone), 0, "change {gone} keeps no refs");
    }
    let (st, _) = http_get(&server.url(&format!("/api/changes/{open}")));
    assert_eq!(st, 200, "the open change stays");
    assert_eq!(keep_refs(&g, open), 1);
}

/// A closed change stays until its last entry is older than the cutoff.
#[test]
fn collect_keeps_a_change_closed_after_the_cutoff() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", a);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let change_number = a_change(&server, &g, "feat");
    let cutoff = Timestamp::now();
    abandon(&server, change_number);

    let deleted = collect(&server, cutoff);

    assert_eq!(deleted, 0);
    let (st, _) = http_get(&server.url(&format!("/api/changes/{change_number}")));
    assert_eq!(st, 200, "the change stays");
}

/// A socket that follows a collected change reads `left` for it.
#[test]
fn collect_sends_left_for_a_followed_change() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", a);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let change_number = a_change(&server, &g, "feat");
    abandon(&server, change_number);
    let mut socket = ws_subscribe(&server, &json!({ "query": { "change": change_number } }));
    assert!(ws_read(&mut socket)["projection"].is_object());

    collect(&server, Timestamp::now() + SignedDuration::from_hours(1));

    assert_eq!(ws_read(&mut socket), json!({ "left": change_number }));
}
