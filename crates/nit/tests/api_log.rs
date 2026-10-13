//! The log of a change over real HTTP: `GET /api/changes/{id}/log`.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, member_id, msg, push};
use serde_json::{Value, json};

fn entries(resp: &Value) -> &Vec<Value> {
    resp["entries"].as_array().unwrap()
}

/// `GET /api/changes/{id}/log` reads the entries of a change, ascending by
/// position, in the range that `from` and `to` select. It 404s for an
/// unknown change.
#[test]
fn change_log_reads_the_entries_of_a_change() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("core: A", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", a);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let a_id = member_id(&res, "Ia");
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{a_id}/comments")),
        &json!({"revision": 0, "body": "note on A"}),
    );
    assert_eq!(st, 200);

    let (st, log) = http_get(&server.url(&format!("/api/changes/{a_id}/log")));
    assert_eq!(st, 200, "{log}");
    let positions: Vec<(u64, &str)> = entries(&log)
        .iter()
        .map(|e| (e["position"].as_u64().unwrap(), e["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(positions, vec![(0, "revision"), (1, "comment")]);

    let (st, log) = http_get(&server.url(&format!("/api/changes/{a_id}/log?from=1")));
    assert_eq!(st, 200, "{log}");
    assert_eq!(entries(&log)[0]["kind"], "comment", "`from` is inclusive");
    assert_eq!(entries(&log).len(), 1);
    let (st, log) = http_get(&server.url(&format!("/api/changes/{a_id}/log?to=1")));
    assert_eq!(st, 200, "{log}");
    assert_eq!(entries(&log)[0]["kind"], "revision", "`to` is exclusive");
    assert_eq!(entries(&log).len(), 1);

    let (st, body) = http_get(&server.url("/api/changes/999/log"));
    assert_eq!(st, 404, "{body}");
}
