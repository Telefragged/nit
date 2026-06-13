//! `/log` range semantics over real HTTP: half-open slices, open-ended
//! ranges through the head, and out-of-dataset queries erroring rather than
//! clamping (docs/api.md "log").

mod common;

use common::{GitRepo, TestServer, http_get, http_post, msg};
use serde_json::json;

fn idxs(resp: &serde_json::Value) -> Vec<i64> {
    resp["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["idx"].as_i64().unwrap())
        .collect()
}

#[test]
fn log_ranges_slice_and_reject_out_of_bounds() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: thing", "Ia"), &[("a.txt", "one\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, chain) = http_post(
        &server.url("/api/chains"),
        &json!({
            "repo_path": g.workdir().to_string_lossy(),
            "branch": "feat",
            "base": "main",
        }),
    );
    assert_eq!(st, 200, "{chain}");
    let chain_id = chain["id"].as_i64().unwrap();
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    // Two reviews on top of the push entry → head 3 (idx 0, 1, 2).
    for _ in 0..2 {
        let (st, _) = http_post(
            &server.url(&format!("/api/changes/{change_id}/reviews")),
            &json!({"revision": 1, "verdict": "comment", "message": "m"}),
        );
        assert_eq!(st, 200);
    }
    let log = |q: &str| http_get(&server.url(&format!("/api/chains/{chain_id}/log?{q}")));

    // Open `..` (no params) → the whole log.
    let (st, all) = log("");
    assert_eq!(st, 200);
    assert_eq!(all["head"], 3);
    assert_eq!(idxs(&all), vec![0, 1, 2]);

    // Half-open [0, 2) and the open tail [1, head).
    let (_, slice) = log("from=0&to=2");
    assert_eq!(idxs(&slice), vec![0, 1]);
    let (_, tail) = log("from=1");
    assert_eq!(idxs(&tail), vec![1, 2]);

    // A bare index → exactly that one entry ([2, 3)).
    let (_, one) = log("from=2&to=3");
    assert_eq!(idxs(&one), vec![2]);

    // Open `from == head` selects nothing — empty list, not an error.
    let (st, empty) = log("from=3");
    assert_eq!(st, 200);
    assert!(empty["entries"].as_array().unwrap().is_empty());

    // Out-of-dataset and reversed ranges are 400s, never clamped.
    assert_eq!(log("from=0&to=10").0, 400); // closed end past head
    assert_eq!(log("from=4").0, 400); // open start past head
    assert_eq!(log("from=2&to=1").0, 400); // reversed
    assert_eq!(log("from=2&to=2").0, 400); // empty
}
