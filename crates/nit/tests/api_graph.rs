//! The spine-centered change graph over HTTP (docs/api.md "Graph"). Two
//! invariants the unit tests can't reach because they live in `build_graph`'s
//! assembly of a live repo: (1) the row-order partition — an open change forking
//! BEHIND the canonical HEAD orders ABOVE the HEAD anchor and keeps its real
//! fork base (the backend never re-roots it onto HEAD); a single global topo
//! would float the childless HEAD to the top instead. (2) `history_truncated`
//! flips exactly at the window boundary.

mod common;

use common::{GitRepo, TestServer, first_repo_id, http_get, msg, push};
use nit::api::MERGED_WINDOW;
use serde_json::Value;

/// GET the repo's graph, asserting 200.
fn get_graph(server: &TestServer, repo_id: u64) -> Value {
    let (st, g) = http_get(&server.url(&format!("/api/repos/{repo_id}/graph")));
    assert_eq!(st, 200, "{g}");
    g
}

/// Row index of the node with `sha`, or panic.
fn row_of(graph: &Value, sha: &str) -> usize {
    graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|n| n["commit_sha"] == sha)
        .unwrap_or_else(|| panic!("no node {sha} in graph: {graph}"))
}

fn node<'a>(graph: &'a Value, sha: &str) -> &'a Value {
    &graph["nodes"].as_array().unwrap()[row_of(graph, sha)]
}

// main advances root→c1→c2→c3 while a topic forks at c1, two commits behind
// HEAD. The open topic must order ABOVE the HEAD anchor (the partition: a
// childless HEAD must not float to row 0) and keep c1 — its real fork point —
// as its parent, never re-rooted onto the anchor.
#[test]
fn open_fork_behind_head_orders_above_anchor_and_keeps_its_base() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("main: one", "Im1"), &[("m1", "1\n")]);
    let c2 = g.commit(&[c1], &msg("main: two", "Im2"), &[("m2", "2\n")]);
    let c3 = g.commit(&[c2], &msg("main: three", "Im3"), &[("m3", "3\n")]);
    g.branch("main", c3); // HEAD advances to c3, leaving the topic behind
    let topic = g.commit(&[c1], &msg("topic: behind HEAD", "Itopic"), &[("t", "t\n")]);
    g.branch("topic", topic);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, res) = push(&server, &g, "topic", "main", None);
    assert_eq!(st, 200, "{res}");
    let repo_id = first_repo_id(&server);
    let graph = get_graph(&server, repo_id);
    let (topic, c1, c3) = (topic.to_string(), c1.to_string(), c3.to_string());

    // The HEAD anchor is main's tip c3; the topic is its own open section.
    assert_eq!(graph["anchor"], c3.as_str());
    assert_eq!(node(&graph, &c3)["section"], "head");
    assert_eq!(node(&graph, &topic)["section"], "open");

    // The partition invariant: the open node precedes the HEAD anchor. A single
    // global topo would float the childless HEAD to row 0 and break this.
    assert!(
        row_of(&graph, &topic) < row_of(&graph, &c3),
        "open fork must order above the HEAD anchor: {graph}"
    );

    // No re-rooting: topic keeps c1 (its real fork, a visible history node) as
    // its parent, not the anchor c3.
    let parents = node(&graph, &topic)["parents"].as_array().unwrap();
    assert_eq!(parents.len(), 1, "{graph}");
    assert_eq!(parents[0], c1.as_str(), "topic keeps its real fork base");
    assert_ne!(parents[0], c3.as_str(), "topic is not re-rooted onto HEAD");
    assert_eq!(node(&graph, &c1)["section"], "history");
}

/// Build a repo whose main has exactly `below` merged commits beneath HEAD,
/// push a topic forked at HEAD, and return the graph's `history_truncated`.
fn truncated_with_history(below: u64) -> bool {
    let g = GitRepo::new();
    // `below` commits on top of the root leaves the root plus `below - 1`
    // intermediates beneath HEAD — `below` merged commits in all.
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
    let (st, res) = push(&server, &g, "topic", "main", None);
    assert_eq!(st, 200, "{res}");
    let repo_id = first_repo_id(&server);
    get_graph(&server, repo_id)["history_truncated"]
        .as_bool()
        .expect("history_truncated is a bool")
}

// `history_truncated` is true exactly when the canonical branch has more merged
// commits below HEAD than the fixed `MERGED_WINDOW` shows: at the window it is
// all visible, one deeper hides the oldest.
#[test]
fn history_truncated_flips_at_the_window_boundary() {
    assert!(
        !truncated_with_history(MERGED_WINDOW),
        "exactly MERGED_WINDOW below HEAD: nothing hidden"
    );
    assert!(
        truncated_with_history(MERGED_WINDOW + 1),
        "one deeper than the window: the oldest is hidden"
    );
}
