//! Ported comments over HTTP: `GET /api/changes/{id}/revisions/{n}/ported`
//! carries every unresolved thread of an earlier revision to `n`.

mod common;

use common::*;
use serde_json::{Value, json};

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

fn line(file: &str, side: &str, at: u64) -> Value {
    json!({"line": {"file": file, "side": side, "at": {"whole": at}}})
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
        &msg("feat: thing", "Iport01"),
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
        &json!({"line": {"file": "a.txt", "side": "new", "at": {"selection": {"start_line": 7, "start_char": 0, "end_line": 8, "end_char": 1}}}}),
        false,
    );
    let rewritten = thread(&server, id, 0, &line("a.txt", "new", 4), false);
    let old_side = thread(&server, id, 0, &line("a.txt", "old", 2), false);
    let file = thread(&server, id, 0, &json!({"file": {"file": "a.txt"}}), false);
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
        &msg("feat: thing, reworded", "Iport01"),
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
    assert_eq!(anchor_of(&p, shifted), line("a.txt", "new", 8));
    assert_eq!(
        anchor_of(&p, selection),
        json!({"line": {"file": "a.txt", "side": "new", "at": {"selection": {"start_line": 9, "start_char": 0, "end_line": 10, "end_char": 1}}}})
    );
    assert_eq!(anchor_of(&p, rewritten), json!({"file": {"file": "a.txt"}}));
    // The parents are the same tree, so the old side did not move.
    assert_eq!(anchor_of(&p, old_side), line("a.txt", "old", 2));
    assert_eq!(anchor_of(&p, file), json!({"file": {"file": "a.txt"}}));
    assert_eq!(anchor_of(&p, change), json!("change"));
    assert_eq!(anchor_of(&p, deleted), json!("change"));
    assert_eq!(
        anchor_of(&p, subject),
        json!({"file": {"file": "/COMMIT_MSG"}})
    );
    let ids: Vec<u64> = p
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["thread_id"].as_u64().unwrap())
        .collect();
    assert!(!ids.contains(&resolved), "a resolved thread is not ported");
    assert!(
        !ids.contains(&later),
        "a thread on the target is not ported"
    );
    assert_eq!(ids.len(), 8);
    assert!(
        p.as_array().unwrap().iter().all(|x| x["revision"] == 1),
        "every entry names the target"
    );

    let (st, e) = ported(&server, id, 9, "");
    assert_eq!(st, 404, "{e}");
}

/// `?against={m}` ports to the interdiff's other tree too: `n`'s entries
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
        let c = g.commit(&[base], &msg("feat: thing", "Iport02"), &[("a.txt", &body)]);
        g.branch("feat", c);
        let (st, pushed) = push(&server, &g, "feat", "main");
        assert_eq!(st, 200, "{pushed}");
        id = tip_change(&pushed)["change_number"].as_u64().unwrap();
        assert_eq!(tip_change(&pushed)["revision"], revision);
        // Each amend inserts one line above everything.
        body.insert_str(0, "a0\n");
    }
    let t = thread(&server, id, 0, &line("a.txt", "new", 6), false);

    let (st, p) = ported(&server, id, 2, "?against=1");
    assert_eq!(st, 200, "{p}");
    let places: Vec<(u64, Value)> = p
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["revision"].as_u64().unwrap(), x["anchor"].clone()))
        .collect();
    assert_eq!(
        places,
        vec![(2, line("a.txt", "new", 8)), (1, line("a.txt", "new", 7))]
    );
    assert_eq!(anchor_of(&p, t), line("a.txt", "new", 8));
}
