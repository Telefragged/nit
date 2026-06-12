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
fn reword_blocks_stale_review_retarget() {
    let g = GitRepo::new();
    let a_txt = "a1\na2\na3\n";
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
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    // The agent rewords the message (same diff, same parent, same
    // trailer): patch-id-equal, but the reviewer never saw this message.
    let c1b = g.commit(
        &[g.root],
        &msg("core: add a, but explained", "Ia"),
        &[("a.txt", a_txt)],
    );
    g.branch("feat", c1b);
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200);
    assert_eq!(chain["changes"][0]["revision"], 2);
    assert_eq!(chain["changes"][0]["status"], "pending");

    // A review against the pre-reword revision must not auto-retarget.
    let (st, e) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": "lgtm"}),
    );
    assert_eq!(st, 409, "{e}");
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("no longer the latest")
    );
}

#[test]
fn commit_msg_comments_port_across_reword() {
    let g = GitRepo::new();
    let a_txt = "a1\na2\n";
    // Message lines: 1 subject, 2 blank, 3/4 body, 5 blank, 6 trailer.
    let c1 = g.commit(
        &[g.root],
        "core: add a\n\nFirst body line.\nSecond body line.\n\nChange-Id: Ia\n",
        &[("a.txt", a_txt)],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let register = json!({
        "repo_path": g.workdir().to_string_lossy(),
        "branch": "feat",
        "base": "main",
    });
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    let drafts_url = server.url(&format!("/api/changes/{change_id}/drafts"));
    let (st, on_body) = http_post(
        &drafts_url,
        &json!({"revision": 1, "file": "/COMMIT_MSG", "line": 4, "body": "second?"}),
    );
    assert_eq!(st, 200, "{on_body}");
    assert_eq!(on_body["line_text"], "Second body line.");
    let on_body_id = on_body["id"].as_i64().unwrap();
    let (st, on_subject) = http_post(
        &drafts_url,
        &json!({"revision": 1, "file": "/COMMIT_MSG", "line": 1, "body": "explain more"}),
    );
    assert_eq!(st, 200);
    let on_subject_id = on_subject["id"].as_i64().unwrap();
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "message nits"}),
    );
    assert_eq!(st, 200);

    // Reword: the subject line changes (outdates its anchor), a line is
    // inserted above "Second body line." (shifts its anchor 4 → 5).
    let c1b = g.commit(
        &[g.root],
        "core: add a, explained\n\nFirst body line.\nAn inserted line.\nSecond body line.\n\n\
         Change-Id: Ia\n",
        &[("a.txt", a_txt)],
    );
    g.branch("feat", c1b);
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200);
    assert_eq!(chain["changes"][0]["revision"], 2);

    let (st, detail) = http_get(&server.url(&format!("/api/changes/{change_id}")));
    assert_eq!(st, 200);
    let comments = detail["comments"].as_array().unwrap();
    let by_id = |id: i64| {
        comments
            .iter()
            .find(|c| c["id"].as_i64() == Some(id))
            .unwrap()
    };
    let body = by_id(on_body_id);
    assert_eq!(body["rendered_line"], 5, "unchanged region shifts");
    assert_eq!(body["outdated"], false);
    let subject = by_id(on_subject_id);
    assert_eq!(subject["rendered_line"], Value::Null);
    assert_eq!(subject["outdated"], true, "edited line goes outdated");
    assert_eq!(subject["line_text"], "core: add a");

    // Served at revision 1, both anchors are where they were written.
    let (_, at_r1) = http_get(&server.url(&format!("/api/changes/{change_id}?revision=1")));
    let comments = at_r1["comments"].as_array().unwrap();
    let find = |id: i64| {
        comments
            .iter()
            .find(|c| c["id"].as_i64() == Some(id))
            .unwrap()
    };
    assert_eq!(find(on_body_id)["rendered_line"], 4);
    assert_eq!(find(on_body_id)["outdated"], false);
    assert_eq!(find(on_subject_id)["rendered_line"], 1);
    assert_eq!(find(on_subject_id)["outdated"], false);
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

    // /COMMIT_MSG drafts: the message has no old side; new-side anchors
    // snapshot message lines (docs/api.md "The commit message as a file").
    let (st, e) = http_post(
        &drafts_url,
        &json!({"revision": 1, "file": "/COMMIT_MSG", "line": 1, "side": "old", "body": "x"}),
    );
    assert_eq!(st, 400, "{e}");
    assert!(e["error"].as_str().unwrap().contains("old side"));
    let (st, msg_draft) = http_post(
        &drafts_url,
        &json!({"revision": 1, "file": "/COMMIT_MSG", "line": 1, "body": "subject nit"}),
    );
    assert_eq!(st, 200, "{msg_draft}");
    assert_eq!(msg_draft["line_text"], "core: x");
    assert_eq!(msg_draft["rendered_line"], 1);

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

#[test]
fn review_rejected_on_orphaned_change() {
    let g = GitRepo::new();
    let seed = g.commit(&[g.root], "seed\n", &[("f.txt", "a\nb\nc\nd\ne\n")]);
    g.branch("main", seed);
    let c1 = g.commit(
        &[seed],
        &msg("one", "I001"),
        &[("f.txt", "A\nb\nc\nd\ne\n")],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let register = json!({
        "repo_path": g.workdir().to_string_lossy(),
        "branch": "feat",
        "base": "main",
    });
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();
    let revision = chain["changes"][0]["revision"].as_i64().unwrap();

    // Orphan the change (reset to base) — verdicts must be rejected.
    g.branch("feat", seed);
    let (st, _) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200);
    let (st, body) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": revision, "verdict": "approve", "message": "ghost"}),
    );
    assert_eq!(st, 409, "reviewing an orphaned change must fail: {body}");
    assert!(body["error"].as_str().unwrap().contains("orphaned"));
}
