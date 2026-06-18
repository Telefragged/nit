//! Log endpoints over real HTTP. `GET /api/changes/{id}/log?from&to` is one
//! change's half-open slice — open ranges through `head`, a valid empty
//! selection, and out-of-dataset queries erroring rather than clamping
//! (docs/api.md "log"). `GET /api/chains/{change_id}/log` aggregates every
//! member's entries, merged and sorted by ascending global `seq`.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, member_id, msg, push};
use serde_json::Value;

/// The `idx` column of a `{entries}` log body, in order.
fn idxs(resp: &Value) -> Vec<u64> {
    entries(resp)
        .iter()
        .map(|e| e["idx"].as_u64().unwrap())
        .collect()
}

/// The `seq` column of a `{entries}` log body, in order.
fn seqs(resp: &Value) -> Vec<u64> {
    entries(resp)
        .iter()
        .map(|e| e["seq"].as_u64().unwrap())
        .collect()
}

fn entries(resp: &Value) -> &Vec<Value> {
    resp["entries"].as_array().unwrap()
}

#[test]
fn change_log_ranges_slice_and_reject_out_of_bounds() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: thing", "Ia"), &[("a.txt", "one\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&res, "Ia");

    // The push wrote one `revision` entry (idx 0). Two reviews on rev 0 take
    // the log to head 3 (idx 0, 1, 2).
    for _ in 0..2 {
        let (st, _) = http_post(
            &server.url(&format!("/api/changes/{change_id}/reviews")),
            &serde_json::json!({"revision": 0, "verdict": "comment", "message": "m"}),
        );
        assert_eq!(st, 200);
    }
    let log = |q: &str| http_get(&server.url(&format!("/api/changes/{change_id}/log?{q}")));

    // No params → the whole log; every entry names this change and `head` is
    // its per-change idx count.
    let (st, all) = log("");
    assert_eq!(st, 200);
    assert_eq!(all["head"], 3);
    assert_eq!(idxs(&all), vec![0, 1, 2]);
    assert!(
        entries(&all)
            .iter()
            .all(|e| e["change_id"].as_u64() == Some(change_id))
    );
    // seq is global-monotone, idx is the per-change position; the first entry
    // is the push's `revision`.
    assert_eq!(all["entries"][0]["kind"], "revision");
    assert!(all["entries"][0]["payload"].is_object());

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
    assert!(entries(&empty).is_empty());

    // Out-of-dataset and reversed/empty ranges are 400s, never clamped.
    assert_eq!(log("from=0&to=10").0, 400); // closed end past head
    assert_eq!(log("from=4").0, 400); // open start past head
    assert_eq!(log("from=2&to=1").0, 400); // reversed
    assert_eq!(log("from=2&to=2").0, 400); // empty
}

#[test]
fn change_log_unknown_change_is_404() {
    let g = GitRepo::new();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, _) = http_get(&server.url("/api/changes/999/log"));
    assert_eq!(st, 404);
}

#[test]
fn chain_log_aggregates_members_in_seq_order() {
    // m → A → B: two changes in one chain. A comment lands on A *between* the
    // two pushes, so the aggregated chain log must interleave it by global
    // `seq`, not group by member.
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("core: A", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", a);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // Push A alone first (seq: A.revision).
    let (st, res) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{res}");
    let a_id = member_id(&res, "Ia");

    // An agent comment on A (seq: A.comment) — written before B exists, so it
    // must sort before B's revision in the merged timeline.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{a_id}/comments")),
        &serde_json::json!({"revision": 0, "body": "note on A"}),
    );
    assert_eq!(st, 200);

    // Extend the chain with B and re-push the tip (seq: B.revision).
    let b = g.commit(&[a], &msg("core: B", "Ib"), &[("b.txt", "b\n")]);
    g.branch("feat", b);
    let (st, res) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{res}");
    let b_id = member_id(&res, "Ib");
    assert_ne!(a_id, b_id);

    // The aggregated chain log: every member's entries merged, sorted by seq.
    let (st, log) = http_get(&server.url(&format!("/api/chains/{b_id}/log")));
    assert_eq!(st, 200, "{log}");

    // Three entries total: A.revision, A.comment, B.revision — strictly
    // ascending seq, and exactly that chronological order.
    let seq = seqs(&log);
    assert_eq!(seq.len(), 3, "{log}");
    assert!(
        seq.windows(2).all(|w| w[0] < w[1]),
        "seq strictly ascending"
    );

    let got: Vec<(u64, &str)> = entries(&log)
        .iter()
        .map(|e| {
            (
                e["change_id"].as_u64().unwrap(),
                e["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![(a_id, "revision"), (a_id, "comment"), (b_id, "revision")],
        "A's two entries precede B's, interleaved by write order"
    );

    // Querying the same chain by its base member's id walks the same tip and
    // yields the identical aggregate (the chain is tip-rooted either way).
    let (st, from_a) = http_get(&server.url(&format!("/api/chains/{a_id}/log")));
    assert_eq!(st, 200, "{from_a}");
    assert_eq!(seqs(&from_a), seq);
}

#[test]
fn chain_log_unknown_change_is_404() {
    let g = GitRepo::new();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, _) = http_get(&server.url("/api/chains/999/log"));
    assert_eq!(st, 404);
}
