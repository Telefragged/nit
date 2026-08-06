//! Change tags over HTTP: what one call puts on a change, what the next
//! one inherits, the `?tag=` filter on the bulk change read, and the tags
//! a repo uses (`GET /api/tags`).

mod common;

use common::{
    GitRepo, TestServer, a_change, change_id, change_ids, change_tags, first_repo_id, get_changes,
    http_get, msg, push, tag_change,
};
use serde_json::{Value, json};

/// Puts `tags` on a change built from `tip`, and returns its number.
fn tagged(server: &TestServer, g: &GitRepo, tip: &str, tags: &Value) -> u64 {
    let number = a_change(server, g, tip);
    let (st, res) = tag_change(server, number, tags);
    assert_eq!(st, 200, "{res}");
    number
}

// The fold lays each `tags` entry over the set it finds. So a second call
// naming one key keeps the keys it omits and replaces the one it names.
#[test]
fn later_tags_overlay_the_ones_before_them() {
    let g = GitRepo::new();
    let one = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic", one);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let number = tagged(
        &server,
        &g,
        "topic",
        &json!({"branch": "topic", "feature": "epic-saga"}),
    );
    let repo_id = first_repo_id(&server);
    assert_eq!(
        change_tags(&server, repo_id, "Ia"),
        json!({"branch": "topic", "feature": "epic-saga"})
    );

    let (st, res) = tag_change(&server, number, &json!({"branch": "renamed"}));
    assert_eq!(st, 200, "{res}");
    assert_eq!(
        change_tags(&server, repo_id, "Ia"),
        json!({"branch": "renamed", "feature": "epic-saga"}),
        "the key nobody named carries forward"
    );
}

// Labelling is its own action, so it needs no push and leaves the
// change's revisions where they were.
#[test]
fn tagging_a_change_records_no_revision() {
    let g = GitRepo::new();
    let one = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic", one);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let number = tagged(&server, &g, "topic", &json!({"branch": "topic"}));
    let (st, res) = tag_change(&server, number, &json!({"feature": "epic-saga"}));
    assert_eq!(st, 200, "{res}");

    let repo_id = first_repo_id(&server);
    assert_eq!(
        change_tags(&server, repo_id, "Ia"),
        json!({"branch": "topic", "feature": "epic-saga"})
    );
    let (st, detail) = http_get(&server.url(&format!("/api/changes/{number}")));
    assert_eq!(st, 200, "{detail}");
    assert_eq!(detail["revisions"].as_array().expect("revisions").len(), 1);
}

// Tags that name nothing new append nothing, so re-running `nit push`
// from the same worktree and branch leaves the log where it was.
#[test]
fn tags_that_move_no_key_append_nothing() {
    let g = GitRepo::new();
    let one = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic", one);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let tags = json!({"branch": "topic", "feature": "epic-saga"});
    let number = tagged(&server, &g, "topic", &tags);
    let before = log_len(&server, number);

    let (st, res) = tag_change(&server, number, &tags);
    assert_eq!(st, 200, "{res}");
    assert_eq!(log_len(&server, number), before);

    let (st, res) = tag_change(&server, number, &json!({"branch": "topic"}));
    assert_eq!(st, 200, "{res}");
    assert_eq!(
        log_len(&server, number),
        before,
        "a subset that matches moves no key either"
    );
}

/// How many entries the change's chain log holds.
fn log_len(server: &TestServer, change_number: u64) -> usize {
    let (st, log) = http_get(&server.url(&format!("/api/chains/{change_number}/log")));
    assert_eq!(st, 200, "{log}");
    log["entries"].as_array().expect("entries").len()
}

// A change nobody has heard of takes no tags.
#[test]
fn tagging_an_unknown_change_is_404() {
    let g = GitRepo::new();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, body) = tag_change(&server, 999, &json!({"track": "perf"}));
    assert_eq!(st, 404, "{body}");
}

#[test]
fn tag_filter_is_exact_and_ands_every_pair() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic-a", a);
    let b = g.commit(&[g.root], &msg("b: two", "Ib"), &[("b", "2\n")]);
    g.branch("topic-b", b);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    tagged(
        &server,
        &g,
        "topic-a",
        &json!({"session-id": "s1", "feature": "epic-saga"}),
    );
    tagged(&server, &g, "topic-b", &json!({"session-id": "s1"}));
    let repo_id = first_repo_id(&server);

    let q = |filter: &str| change_ids(&get_changes(&server, &format!("?repo={repo_id}{filter}")));
    assert_eq!(q(""), vec![change_id("Ia"), change_id("Ib")]);
    assert_eq!(
        q("&tag=session-id=s1"),
        vec![change_id("Ia"), change_id("Ib")]
    );
    assert_eq!(
        q("&tag=session-id=s1&tag=feature=epic-saga"),
        vec![change_id("Ia")],
        "every requested tag must match"
    );
    assert!(q("&tag=feature=epic").is_empty(), "no prefix matching");
    assert!(q("&tag=session-id=s2").is_empty());
}

#[test]
fn repo_tags_group_every_value_under_its_key() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic-a", a);
    let b = g.commit(&[g.root], &msg("b: two", "Ib"), &[("b", "2\n")]);
    g.branch("topic-b", b);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    tagged(
        &server,
        &g,
        "topic-a",
        &json!({"track": "perf", "session-id": "s1"}),
    );
    tagged(&server, &g, "topic-b", &json!({"track": "docs"}));
    let repo_id = first_repo_id(&server);

    let (st, body) = http_get(&server.url(&format!("/api/tags?repo={repo_id}")));
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        body["tags"],
        json!({"session-id": ["s1"], "track": ["docs", "perf"]})
    );
}

// An unknown repo narrows to nothing rather than returning 404, which
// matches `?repo=` on the change read.
#[test]
fn tags_of_an_unknown_repo_are_empty() {
    let g = GitRepo::new();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, body) = http_get(&server.url("/api/tags?repo=999"));
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["tags"], json!({}));
}

// The key vocabulary is enforced at both boundaries a tag crosses.
#[test]
fn a_malformed_tag_is_rejected() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic", a);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let number = a_change(&server, &g, "topic");
    let (st, res) = tag_change(&server, number, &json!({"-bad": "v"}));
    assert_eq!(st, 400, "{res}");
    let (st, res) = tag_change(&server, number, &json!({"ok": ""}));
    assert_eq!(st, 400, "{res}");
    let (st, res) = tag_change(&server, number, &json!({"ok": "v"}));
    assert_eq!(st, 200, "{res}");

    let repo_id = first_repo_id(&server);
    // A query pair without `=` is not a tag, so the query fails to deserialize.
    let (st, body) = http_get(&server.url(&format!("/api/changes?repo={repo_id}&tag=novalue")));
    assert_eq!(st, 400, "{body}");
}

#[test]
fn tags_survive_a_restart() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("tagged", a);
    let b = g.commit(&[g.root], &msg("b: two", "Ib"), &[("b", "2\n")]);
    g.branch("untagged", b);
    let db = g.dir.path().join("nit.sqlite3");

    let repo_id = {
        let server = TestServer::start(db.clone(), None);
        tagged(&server, &g, "tagged", &json!({"track": "perf"}));
        let (st, res) = push(&server, &g, "untagged", "main");
        assert_eq!(st, 200, "{res}");
        first_repo_id(&server)
    };

    let server = TestServer::start(db, None);
    let matched = get_changes(&server, &format!("?repo={repo_id}&tag=track=perf"));
    assert_eq!(change_ids(&matched), vec![change_id("Ia")]);
    assert_eq!(
        change_ids(&get_changes(&server, &format!("?repo={repo_id}"))),
        vec![change_id("Ia"), change_id("Ib")]
    );
    assert_eq!(change_tags(&server, repo_id, "Ib"), Value::Null);
}
