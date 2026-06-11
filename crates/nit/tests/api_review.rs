//! Review submission semantics (docs/api.md "Reviews") plus request
//! validation: pure-rebase auto-retarget, status preservation across pure
//! rebases, feedback scoping of unresolved older threads, and the 400s.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, msg};
use serde_json::{Value, json};

#[test]
fn review_auto_retargets_after_pure_rebase() {
    let g = GitRepo::new();
    let a_txt = "a1\na2\na3\na4\na5\n";
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", a_txt)]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let register = json!({
        "repo_path": g.workdir().to_string_lossy(),
        "branch": "feat",
        "base": "main",
    });
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    let chain_id = chain["id"].as_i64().unwrap();
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    // Reviewer asks a question on revision 1 (leaves an unresolved thread).
    let (st, draft) = http_post(
        &server.url(&format!("/api/changes/{change_id}/drafts")),
        &json!({"revision": 1, "file": "a.txt", "line": 2, "body": "why a2?"}),
    );
    assert_eq!(st, 200);
    let root_id = draft["id"].as_i64().unwrap();
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "comment", "message": "one question"}),
    );
    assert_eq!(st, 200);

    // The agent rebases onto a moved base: same diff, new sha (pure rebase).
    let m1 = g.commit(&[g.root], "mainline: unrelated\n", &[("b.txt", "b\n")]);
    g.branch("main", m1);
    let c1r = g.commit(&[m1], &msg("core: add a", "Ia"), &[("a.txt", a_txt)]);
    g.branch("feat", c1r);

    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200);
    let ch = &chain["changes"][0];
    assert_eq!(ch["revision"], 2);
    assert_eq!(ch["commit_sha"], c1r.to_string());
    assert_eq!(
        ch["status"], "commented",
        "a pure rebase must keep the review status"
    );

    // Submitting against the stale revision 1 auto-retargets to 2.
    let (st, submitted) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": "lgtm"}),
    );
    assert_eq!(st, 200, "{submitted}");
    assert_eq!(submitted["review"]["revision"], 2);

    let (_, feedback) = http_get(&server.url(&format!("/api/chains/{chain_id}/feedback")));
    assert_eq!(feedback["state"], "ready_to_merge");
    assert_eq!(feedback["changes"][0]["status"], "approved");
    // The latest review (approve) has no comments, but the unresolved
    // thread from the earlier review stays in feedback scope.
    let comments = feedback["changes"][0]["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["id"].as_i64(), Some(root_id));
    assert_eq!(comments[0]["resolved"], false);
}

#[test]
fn request_validation() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
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
    assert_eq!(st, 200);
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();
    let drafts_url = server.url(&format!("/api/changes/{change_id}/drafts"));
    let reviews_url = server.url(&format!("/api/changes/{change_id}/reviews"));

    // Drafts.
    let cases: &[(Value, &str)] = &[
        (json!({"revision": 9, "body": "x"}), "unknown revision"),
        (
            json!({"revision": 1, "file": "x.txt", "line": 1, "side": "sideways", "body": "x"}),
            "bad side",
        ),
        (
            json!({"revision": 1, "line": 3, "body": "x"}),
            "line without file",
        ),
        (
            json!({"revision": 1, "body": "x", "parent_id": 999}),
            "unknown parent",
        ),
    ];
    for (body, what) in cases {
        let (st, e) = http_post(&drafts_url, body);
        assert!((400..=404).contains(&st), "{what}: {st} {e}");
    }

    // Reviews: bad verdict, unknown revision.
    let (st, e) = http_post(
        &reviews_url,
        &json!({"revision": 1, "verdict": "maybe", "message": ""}),
    );
    assert_eq!(st, 400, "{e}");
    let (st, e) = http_post(
        &reviews_url,
        &json!({"revision": 9, "verdict": "approve", "message": ""}),
    );
    assert_eq!(st, 400, "{e}");

    // Comments: resolving a draft is a 400; replying to a draft is a 404;
    // resolving a reply is a 400 (root comments only).
    let (_, draft) = http_post(
        &drafts_url,
        &json!({"revision": 1, "file": "x.txt", "line": 1, "body": "root"}),
    );
    let draft_id = draft["id"].as_i64().unwrap();
    let (st, _) = http_post(
        &server.url(&format!("/api/comments/{draft_id}/resolve")),
        &json!({}),
    );
    assert_eq!(st, 400);
    let (st, _) = http_post(
        &server.url(&format!("/api/comments/{draft_id}/replies")),
        &json!({"body": "hi"}),
    );
    assert_eq!(st, 404);

    let (st, _) = http_post(&reviews_url, &json!({"revision": 1, "verdict": "comment"}));
    assert_eq!(st, 200);
    let (st, reply) = http_post(
        &server.url(&format!("/api/comments/{draft_id}/replies")),
        &json!({"body": "answer"}),
    );
    assert_eq!(st, 200);
    let reply_id = reply["id"].as_i64().unwrap();
    let (st, e) = http_post(
        &server.url(&format!("/api/comments/{reply_id}/resolve")),
        &json!({}),
    );
    assert_eq!(st, 400, "{e}");

    // Resolve/unresolve roundtrip on the published root.
    let (st, resolved) = http_post(
        &server.url(&format!("/api/comments/{draft_id}/resolve")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(resolved["resolved"], true);
    let (st, unresolved) = http_post(
        &server.url(&format!("/api/comments/{draft_id}/unresolve")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(unresolved["resolved"], false);
}
