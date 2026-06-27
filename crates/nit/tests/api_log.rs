//! The aggregated chain log over real HTTP: `GET /api/chains/{change_id}/log`
//! merges every member's entries, sorted by ascending global `seq`.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, member_id, msg, push};
use serde_json::Value;

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
fn chain_log_aggregates_members_in_seq_order() {
    // m → A → B: two changes in one chain. A comment lands on A *between* the
    // two pushes, so the aggregated chain log must interleave it by global
    // `seq`, not group by member.
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("core: A", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", a);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // This push contributes the A.revision entry.
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let a_id = member_id(&server, &res, "Ia");

    // An agent comment on A (seq: A.comment) — written before B exists, so it
    // must sort before B's revision in the merged timeline.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{a_id}/comments")),
        &serde_json::json!({"revision": 0, "body": "note on A"}),
    );
    assert_eq!(st, 200);

    // This push contributes the B.revision entry.
    let b = g.commit(&[a], &msg("core: B", "Ib"), &[("b.txt", "b\n")]);
    g.branch("feat", b);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let b_id = member_id(&server, &res, "Ib");
    assert_ne!(a_id, b_id);

    let (st, log) = http_get(&server.url(&format!("/api/chains/{b_id}/log")));
    assert_eq!(st, 200, "{log}");

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

    // The comment opened a new thread, so the append minted its id (0) and
    // stamped it into the stored payload — readers need no replay to name it.
    let comment = entries(&log)
        .iter()
        .find(|e| e["kind"] == "comment")
        .expect("the comment entry");
    assert_eq!(
        comment["payload"]["thread_id"],
        serde_json::json!(0),
        "the comment names its minted thread in the payload"
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
