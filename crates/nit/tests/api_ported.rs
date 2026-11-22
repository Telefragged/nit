//! Ported comments over HTTP: `GET /api/changes/{id}/revisions/{n}/ported`
//! carries every thread of an earlier revision to `n`, resolved or not.

mod common;

use common::*;
use serde_json::{Value, json};

const IPORT01: &str = "I706f727430310000000000000000000000000000";
const IPORT02: &str = "I706f727430320000000000000000000000000000";

/// Opens an author thread and returns its id.
fn thread(server: &TestServer, id: u64, revision: u64, anchor: &Value, resolved: bool) -> u64 {
    let (st, t) = http_post(
        &server.url(&format!("/api/changes/{id}/comments")),
        &json!({"revision": revision, "anchor": anchor, "body": "note", "resolved": resolved}),
    );
    assert_eq!(st, 200, "{t}");
    t["id"].as_u64().unwrap()
}

fn ported(server: &TestServer, id: u64, revision: u64, query: &str) -> (u16, Value) {
    http_get(&server.url(&format!(
        "/api/changes/{id}/revisions/{revision}/ported{query}"
    )))
}

fn anchor_of(ported: &Value, thread_id: u64) -> Value {
    ported
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["thread_id"].as_u64() == Some(thread_id))
        .unwrap_or_else(|| panic!("thread {thread_id} not ported: {ported}"))["anchor"]
        .clone()
}

fn whole(file: &str, side: &str) -> Value {
    json!({"file": {"path": file, "side": side, "anchor": "file"}})
}

fn line(file: &str, side: &str, at: u64) -> Value {
    json!({"file": {"path": file, "side": side, "anchor": {"line": at}}})
}

/// A ported line anchor on line `at` of `file` in `version`.
fn ported_line(version: &str, file: &str, at: u64) -> Value {
    json!({"file": {"version": version, "path": file, "anchor": {"line": at}}})
}

/// Each anchor lands where its lines went: shifted past an insertion,
/// dropped to the file where its line was rewritten, to the change where
/// its file was deleted, and untouched on a tree the amend left alone.
#[test]
fn threads_of_earlier_revisions_port_to_the_target_trees() {
    let g = GitRepo::new();
    let base = g.commit(
        &[g.root],
        "base\n",
        &[("a.txt", &lines("a", 1..=8)), ("b.txt", "b1\n")],
    );
    g.branch("main", base);

    let c1 = g.commit(
        &[base],
        &msg("feat: thing", IPORT01),
        &[
            ("a.txt", &lines("a", 1..=8).replace("a4\n", "a4 v1\n")),
            ("b.txt", "b1 v1\n"),
        ],
    );
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change(&pushed)["change_number"].as_u64().unwrap();

    let shifted = thread(&server, id, 0, &line("a.txt", "new", 6), false);
    let selection = thread(
        &server,
        id,
        0,
        &json!({"file": {"path": "a.txt", "side": "new", "anchor": {"range": {"start_line": 7, "start_char": 0, "end_line": 8, "end_char": 1}}}}),
        false,
    );
    let rewritten = thread(&server, id, 0, &line("a.txt", "new", 4), false);
    let old_side = thread(&server, id, 0, &line("a.txt", "old", 2), false);
    let file = thread(&server, id, 0, &whole("a.txt", "new"), false);
    let old_file = thread(&server, id, 0, &whole("a.txt", "old"), false);
    let change = thread(&server, id, 0, &json!("change"), false);
    let deleted = thread(&server, id, 0, &line("b.txt", "new", 1), false);
    let subject = thread(&server, id, 0, &line("/COMMIT_MSG", "new", 1), false);
    let resolved = thread(&server, id, 0, &line("a.txt", "new", 1), true);

    // Nothing is earlier than revision 0.
    let (st, none) = ported(&server, id, 0, "");
    assert_eq!(st, 200, "{none}");
    assert_eq!(none, json!([]));

    // Revision 1: two lines inserted above everything, a4 rewritten again,
    // b.txt deleted, the subject reworded.
    let mut a_v2 = lines("a", 1..=8).replace("a4\n", "a4 v2\n");
    a_v2.insert_str(0, "a0a\na0b\n");
    let c2 = g.commit_full(
        &[base],
        &msg("feat: thing, reworded", IPORT01),
        &[("a.txt", a_v2.as_bytes())],
        &["b.txt"],
    );
    g.branch("feat", c2);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    assert_eq!(tip_change(&pushed)["revision"], 1);
    let later = thread(&server, id, 1, &line("a.txt", "new", 1), false);

    let (st, p) = ported(&server, id, 1, "");
    assert_eq!(st, 200, "{p}");
    assert_eq!(anchor_of(&p, shifted), ported_line("r1", "a.txt", 8));
    assert_eq!(
        anchor_of(&p, selection),
        json!({"file": {"version": "r1", "path": "a.txt", "anchor": {"range": {"start_line": 9, "start_char": 0, "end_line": 10, "end_char": 1}}}})
    );
    assert_eq!(
        anchor_of(&p, rewritten),
        json!({"file": {"version": "r1", "path": "a.txt", "anchor": "file"}})
    );
    // The parents are the same tree, so the old side did not move.
    assert_eq!(anchor_of(&p, old_side), ported_line("r1^", "a.txt", 2));
    assert_eq!(
        anchor_of(&p, file),
        json!({"file": {"version": "r1", "path": "a.txt", "anchor": "file"}})
    );
    assert_eq!(
        anchor_of(&p, old_file),
        json!({"file": {"version": "r1^", "path": "a.txt", "anchor": "file"}})
    );
    assert_eq!(anchor_of(&p, change), json!({"change": {"revision": 1}}));
    assert_eq!(anchor_of(&p, deleted), json!({"change": {"revision": 1}}));
    assert_eq!(
        anchor_of(&p, subject),
        json!({"file": {"version": "r1", "path": "/COMMIT_MSG", "anchor": "file"}})
    );
    assert_eq!(anchor_of(&p, resolved), ported_line("r1", "a.txt", 3));
    let ids: Vec<u64> = p
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["thread_id"].as_u64().unwrap())
        .collect();
    assert!(
        !ids.contains(&later),
        "a thread on the target is not ported"
    );
    assert_eq!(ids.len(), 10);

    let (st, e) = ported(&server, id, 9, "");
    assert_eq!(st, 404, "{e}");
}

#[test]
fn a_region_or_an_element_ports_only_through_unchanged_bytes() {
    let g = GitRepo::new();
    let base = g.commit(
        &[g.root],
        "base\n",
        &[("page.html", "<p>a</p>\n"), ("shot.png", "png 0\n")],
    );
    g.branch("main", base);
    let c1 = g.commit(
        &[base],
        &msg("feat: thing", IPORT01),
        &[("page.html", "<p>b</p>\n"), ("shot.png", "png 1\n")],
    );
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change(&pushed)["change_number"].as_u64().unwrap();

    let element = |side| json!({"file": {"path": "page.html", "side": side, "anchor": {"element": "/html/body/p"}}});
    let region = json!({"file": {"path": "shot.png", "side": "new", "anchor": {"region": {"rect": {"x": 1, "y": 2, "width": 3, "height": 4}}}}});
    let edited = thread(&server, id, 0, &element("new"), false);
    let old_side = thread(&server, id, 0, &element("old"), false);
    let kept = thread(&server, id, 0, &region, false);

    let c2 = g.commit(
        &[base],
        &msg("feat: thing", IPORT01),
        &[("page.html", "<p>c</p>\n"), ("shot.png", "png 1\n")],
    );
    g.branch("feat", c2);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");

    let (st, p) = ported(&server, id, 1, "");
    assert_eq!(st, 200, "{p}");
    assert_eq!(
        anchor_of(&p, edited),
        json!({"file": {"version": "r1", "path": "page.html", "anchor": "file"}})
    );
    assert_eq!(
        anchor_of(&p, old_side),
        json!({"file": {"version": "r1^", "path": "page.html", "anchor": {"element": "/html/body/p"}}})
    );
    assert_eq!(
        anchor_of(&p, kept),
        json!({"file": {"version": "r1", "path": "shot.png", "anchor": {"region": {"rect": {"x": 1, "y": 2, "width": 3, "height": 4}}}}})
    );
}

/// `?against=r{m}` ports to the interdiff's other tree too: `n`'s entries
/// first, then `m`'s, each thread at its place in that revision.
#[test]
fn against_ports_to_both_revisions() {
    let g = GitRepo::new();
    let base = g.commit(&[g.root], "base\n", &[("a.txt", &lines("a", 1..=8))]);
    g.branch("main", base);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let mut body = lines("a", 1..=8);
    let mut id = 0;
    for revision in 0..3 {
        let c = g.commit(&[base], &msg("feat: thing", IPORT02), &[("a.txt", &body)]);
        g.branch("feat", c);
        let (st, pushed) = push(&server, &g, "feat", "main");
        assert_eq!(st, 200, "{pushed}");
        id = tip_change(&pushed)["change_number"].as_u64().unwrap();
        assert_eq!(tip_change(&pushed)["revision"], revision);
        // Each amend inserts one line above everything.
        body.insert_str(0, "a0\n");
    }
    let t = thread(&server, id, 0, &line("a.txt", "new", 6), false);

    let (st, p) = ported(&server, id, 2, "?against=r1");
    assert_eq!(st, 200, "{p}");
    let places: Vec<Value> = p
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["anchor"].clone())
        .collect();
    assert_eq!(
        places,
        vec![ported_line("r2", "a.txt", 8), ported_line("r1", "a.txt", 7)]
    );
    assert_eq!(anchor_of(&p, t), ported_line("r2", "a.txt", 8));
}
