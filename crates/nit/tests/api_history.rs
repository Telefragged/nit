//! The canonical-history read (`GET /api/history?repo={id}`): the fixed
//! window's truncation boundary, and the `Change-Id` enrichment — a commit
//! whose trailer names a known change carries it, any other trailer nulls
//! both (the coupled-null contract).

mod common;

use common::{GitRepo, TestServer, first_repo_id, http_get, msg, push};
use nit::api::MERGED_WINDOW;
use serde_json::Value;

fn get_history(server: &TestServer, repo_id: u64) -> Value {
    let (st, h) = http_get(&server.url(&format!("/api/history?repo={repo_id}")));
    assert_eq!(st, 200, "{h}");
    h
}

/// A repo with exactly `below` merged commits beneath HEAD and one pushed
/// topic, and its `/api/history` response.
fn history_with(below: u64) -> Value {
    let g = GitRepo::new();
    let mut head = g.root;
    for i in 0..below {
        head = g.commit(
            &[head],
            &msg(&format!("main: {i}"), &format!("Im{i}")),
            &[("m", "x\n")],
        );
    }
    g.branch("main", head);
    let topic = g.commit(&[head], &msg("topic: at HEAD", "Itopic"), &[("t", "t\n")]);
    g.branch("topic", topic);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "topic", "main");
    assert_eq!(st, 200, "{res}");
    let repo_id = first_repo_id(&server);
    get_history(&server, repo_id)
}

// `truncated` flips exactly when the branch has more merged commits below
// HEAD than the fixed window shows.
#[test]
fn truncated_flips_at_the_window_boundary() {
    let at = history_with(MERGED_WINDOW);
    assert_eq!(
        at["commits"].as_array().expect("commits").len() as u64,
        MERGED_WINDOW + 1,
        "HEAD plus the full window"
    );
    assert_eq!(at["truncated"], false, "nothing hidden at the window");
    let deeper = history_with(MERGED_WINDOW + 1);
    assert_eq!(deeper["truncated"], true, "one deeper hides the oldest");
}

// A history commit whose trailer names a known change is enriched with it; a
// trailer naming no change (pre-nit history) nulls both id and key.
#[test]
fn trailer_enrichment_couples_id_and_key() {
    let g = GitRepo::new();
    // Root carries a trailer no change will ever match (pre-nit history).
    let c1 = g.commit(&[g.root], &msg("main: one", "Iforeign"), &[("m", "1\n")]);
    g.branch("main", c1);
    let topic = g.commit(&[c1], &msg("topic: change", "Itopic"), &[("t", "t\n")]);
    g.branch("topic", topic);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "topic", "main");
    assert_eq!(st, 200, "{res}");

    // Land the change: main advances to the pushed commit, and the sweep
    // records the merge by Change-Id.
    g.branch("main", topic);
    common::sweep(&server);

    let repo_id = first_repo_id(&server);
    let history = get_history(&server, repo_id);
    let commits = history["commits"].as_array().expect("commits");
    assert_eq!(commits.len(), 3, "{history}");

    let merged = &commits[0];
    assert_eq!(merged["sha"], topic.to_string());
    assert_eq!(merged["change_key"], "Itopic", "merged change enriched");
    assert!(merged["change_id"].is_u64(), "{merged}");

    let foreign = &commits[1];
    assert_eq!(foreign["sha"], c1.to_string());
    assert!(foreign["change_id"].is_null(), "{foreign}");
    assert!(
        foreign["change_key"].is_null(),
        "a foreign trailer nulls both: {foreign}"
    );
}

#[test]
fn unknown_repo_is_404() {
    let g = GitRepo::new();
    let topic = g.commit(&[g.root], &msg("topic: t", "It"), &[("t", "t\n")]);
    g.branch("topic", topic);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "topic", "main");
    assert_eq!(st, 200, "{res}");
    let (st, _) = http_get(&server.url("/api/history?repo=999"));
    assert_eq!(st, 404);
}
