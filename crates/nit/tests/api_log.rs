//! The log over real HTTP. `GET /api/log` reads the entries of every
//! change a filter matches, sorted by ascending global `sequence`.

mod common;

use common::{
    GitRepo, TestServer, first_repo_id, http_get, http_post, member_id, msg, push, tag_change,
};
use serde_json::{Value, json};

fn seqs(resp: &Value) -> Vec<u64> {
    entries(resp)
        .iter()
        .map(|e| e["sequence"].as_u64().unwrap())
        .collect()
}

fn entries(resp: &Value) -> &Vec<Value> {
    resp["entries"].as_array().unwrap()
}

/// Two changes on separate branches, one tagged. The read returns every
/// entry of the tagged change, the comment written before the tag
/// included. `after` and `before` bound the sequence.
#[test]
fn log_by_tag_reads_the_matched_changes_past_a_cursor() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("core: A", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", a);
    let b = g.commit(&[g.root], &msg("core: B", "Ib"), &[("b.txt", "b\n")]);
    g.branch("other", b);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let a_id = member_id(&res, "Ia");
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{a_id}/comments")),
        &json!({"revision": 0, "body": "note on A"}),
    );
    assert_eq!(st, 200);
    let (st, res) = push(&server, &g, "other", "main");
    assert_eq!(st, 200, "{res}");
    let (st, res) = tag_change(&server, a_id, &json!({"branch": "feat"}));
    assert_eq!(st, 200, "{res}");

    let repo_id = first_repo_id(&server);
    let read = |query: &str| {
        let (st, log) = http_get(&server.url(&format!("/api/log?repo={repo_id}{query}")));
        assert_eq!(st, 200, "{log}");
        log
    };
    let tagged = read("&tag=branch=feat");
    let kinds: Vec<(u64, &str)> = entries(&tagged)
        .iter()
        .map(|e| {
            (
                e["change_number"].as_u64().unwrap(),
                e["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![(a_id, "revision"), (a_id, "comment"), (a_id, "tags")],
        "only the tagged change, every entry it has, in sequence order"
    );
    // The comment opened a new thread, so the append minted its id (0) and
    // stamped it into the stored payload — readers need no replay to name it.
    assert_eq!(
        entries(&tagged)[1]["payload"]["thread_id"],
        json!(0),
        "the comment names its minted thread in the payload"
    );

    let whole_repo = read("");
    assert_eq!(seqs(&whole_repo).len(), 4, "{whole_repo}");
    assert!(seqs(&whole_repo).windows(2).all(|w| w[0] < w[1]));

    let cursor = seqs(&tagged)[0];
    let later = read(&format!("&tag=branch=feat&after={cursor}"));
    assert_eq!(seqs(&later), seqs(&tagged)[1..], "`after` is exclusive");
    let (middle, last) = (seqs(&tagged)[1], seqs(&tagged)[2]);
    let bounded = read(&format!("&tag=branch=feat&after={cursor}&before={last}"));
    assert_eq!(seqs(&bounded), vec![middle], "`before` is exclusive");

    let (st, body) = http_get(&server.url("/api/log"));
    assert_eq!(st, 400, "the repo is required: {body}");
    let (st, body) = http_get(&server.url("/api/log?repo=1&tag=branch"));
    assert_eq!(st, 400, "a tag is spelled key=value: {body}");
}
