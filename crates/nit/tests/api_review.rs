//! The drafts + comments + review flow over HTTP (docs/api.md "Comments",
//! "Reviewer decisions", "Agent endpoints"). A change owns its
//! threads/drafts/reviews; comment drafts are reviewer-private until a staged
//! decision's chain submit (`common::review`) drains them into one log entry,
//! sets the per-(change, revision) status to the verdict, and applies each
//! thread's staged resolution in draft order. Revisions are 0-based: the first
//! push is rev 0, an amend is rev 1.

mod common;

use common::*;
use serde_json::{Value, json};
/// Push `tip` (base `main`) and return its `change_id` (for single-commit
/// chains the tip change is the repo's first change).
fn push_one(server: &TestServer, g: &GitRepo, tip: &str, change_key: &str) -> u64 {
    let (st, res) = push(server, g, tip, "main", None);
    assert_eq!(st, 200, "{res}");
    member_id(&res, change_key)
}

fn drafts_url(server: &TestServer, change_id: u64) -> String {
    server.url(&format!("/api/changes/{change_id}/drafts"))
}

fn detail(server: &TestServer, change_id: u64) -> Value {
    let (st, d) = http_get(&server.url(&format!("/api/changes/{change_id}")));
    assert_eq!(st, 200, "{d}");
    d
}

/// The published thread by fold id, from a fresh change detail.
fn thread_of(server: &TestServer, change_id: u64, thread_id: u64) -> Value {
    detail(server, change_id)["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"].as_u64() == Some(thread_id))
        .cloned()
        .unwrap_or_else(|| panic!("thread {thread_id} not on change {change_id}"))
}

// ---------------------------------------------------------------------------
// Drafts → review → published threads

/// A draft opens a new thread; submitting a review drains it into one review,
/// publishes the thread, and sets the (change, revision) status to the verdict.
#[test]
fn review_drains_drafts_and_sets_status() {
    let g = GitRepo::new();
    let c1 = g.commit(
        &[g.root],
        &msg("core: add a", "Ia"),
        &[("a.txt", "a1\na2\na3\n")],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    // Two new-thread drafts on rev 0; private until published.
    let (st, d1) = http_post(
        &drafts_url(&server, id),
        &json!({"revision": 0, "file": "a.txt", "line": 2, "body": "why a2?"}),
    );
    assert_eq!(st, 200, "{d1}");
    assert_eq!(d1["revision"], 0);
    assert_eq!(d1["line_text"], "a2");
    let (st, _) = http_post(
        &drafts_url(&server, id),
        &json!({"revision": 0, "body": "overall: looks fine"}),
    );
    assert_eq!(st, 200);

    let pre = detail(&server, id);
    assert_eq!(pre["drafts"].as_array().unwrap().len(), 2, "still drafts");
    assert!(pre["threads"].as_array().unwrap().is_empty(), "unpublished");
    assert!(pre["reviews"].as_array().unwrap().is_empty());

    // Stage the verdict and submit the chain (the only publish path): the
    // drafts drain into one review on this change.
    let out = review(&server, id, "request_changes", "a few nits");
    assert_eq!(out["submitted"], 1, "{out}");

    let post = detail(&server, id);
    assert!(
        post["drafts"].as_array().unwrap().is_empty(),
        "drafts drained"
    );
    assert_eq!(post["threads"].as_array().unwrap().len(), 2);
    assert_eq!(post["reviews"].as_array().unwrap().len(), 1);
    assert_eq!(post["reviews"][0]["revision"], 0);
    assert_eq!(post["reviews"][0]["verdict"], "request_changes");
    assert_eq!(post["reviews"][0]["message"], "a few nits");
    // The verdict is the displayed status at (change, rev 0): visible on the
    // path (the thread count is read from the change snapshot, above).
    let (_, chain) = http_get(&server.url(&format!("/api/chains/{id}")));
    let member = chain["path"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["change_id"].as_u64() == Some(id))
        .unwrap();
    assert_eq!(member["status"], "changes_requested");
}

/// A reply draft (`thread_id`) appends to a published thread, keeping the
/// thread's own anchor; submitting a second review publishes the reply.
#[test]
fn reply_draft_appends_to_thread() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x1\nx2\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ix");

    // Open + publish a thread.
    let (_, _) = http_post(
        &drafts_url(&server, id),
        &json!({"revision": 0, "file": "x.txt", "line": 1, "body": "root question"}),
    );
    review(&server, id, "comment", "");
    let thread_id = detail(&server, id)["threads"][0]["id"].as_u64().unwrap();

    // A reply draft references the thread; anchor fields are owned by the thread.
    let (st, reply) = http_post(
        &drafts_url(&server, id),
        &json!({"revision": 0, "thread_id": thread_id, "body": "a follow-up"}),
    );
    assert_eq!(st, 200, "{reply}");
    assert_eq!(reply["thread_id"], thread_id);
    review(&server, id, "comment", "");
    // The published reply lands in the same thread.
    let t = thread_of(&server, id, thread_id);
    let comments = t["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0]["body"], "root question");
    assert_eq!(comments[1]["body"], "a follow-up");
    assert!(comments.iter().all(|c| !c["review_id"].is_null()));
}

// ---------------------------------------------------------------------------
// Range drafts + /COMMIT_MSG side rules

/// A well-formed range round-trips verbatim; the "Range comments" 400s and the
/// `/COMMIT_MSG` old-side 400 are all rejected.
#[test]
fn draft_anchor_validation() {
    let g = GitRepo::new();
    let c1 = g.commit(
        &[g.root],
        "core: x\n\nbody line.\n\nChange-Id: Ix\n",
        &[("x.txt", "x1\nx2\nx3\n")],
    );
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ix");
    let url = drafts_url(&server, id);

    // A well-formed range stores exactly as written.
    let (st, ranged) = http_post(
        &url,
        &json!({"revision": 0, "file": "x.txt", "line": 1, "body": "sel",
                "range": {"start_line": 1, "start_char": 0, "end_line": 1, "end_char": 2}}),
    );
    assert_eq!(st, 200, "{ranged}");
    assert_eq!(
        ranged["range"],
        json!({"start_line": 1, "start_char": 0, "end_line": 1, "end_char": 2})
    );

    // The "Range comments" 400s of docs/api.md.
    let range_400s: &[(Value, &str)] = &[
        (
            json!({"revision": 0, "file": "x.txt", "body": "x",
                   "range": {"start_line": 1, "start_char": 0, "end_line": 1, "end_char": 1}}),
            "range without a line anchor",
        ),
        (
            json!({"revision": 0, "file": "x.txt", "line": 2, "body": "x",
                   "range": {"start_line": 1, "start_char": 0, "end_line": 1, "end_char": 1}}),
            "range.end_line != line",
        ),
        (
            json!({"revision": 0, "file": "x.txt", "line": 1, "body": "x",
                   "range": {"start_line": 1, "start_char": 3, "end_line": 1, "end_char": 3}}),
            "empty range",
        ),
        (
            json!({"revision": 0, "file": "x.txt", "line": 1, "body": "x",
                   "range": {"start_line": 2, "start_char": 0, "end_line": 1, "end_char": 1}}),
            "backwards range",
        ),
        (
            json!({"revision": 0, "file": "x.txt", "line": 2, "body": "x",
                   "range": {"start_line": 1, "start_char": 0, "end_line": 2, "end_char": 0}}),
            "multi-line range ending before its last line's first char",
        ),
    ];
    for (body, what) in range_400s {
        let (st, e) = http_post(&url, body);
        assert_eq!(st, 400, "{what}: {st} {e}");
    }

    // Other anchor faults: unknown revision, a line without a file.
    let (st, _) = http_post(&url, &json!({"revision": 9, "body": "x"}));
    assert_eq!(st, 400, "unknown revision");
    let (st, _) = http_post(&url, &json!({"revision": 0, "line": 3, "body": "x"}));
    assert_eq!(st, 400, "line without file");
    // An empty body is rejected unless a thread_id stages a resolution.
    let (st, _) = http_post(&url, &json!({"revision": 0, "body": ""}));
    assert_eq!(st, 400, "empty body, no resolution");

    // /COMMIT_MSG: old-side anchors are rejected; new-side line 1 is the subject.
    let (st, e) = http_post(
        &url,
        &json!({"revision": 0, "file": "/COMMIT_MSG", "line": 1, "side": "old", "body": "x"}),
    );
    assert_eq!(st, 400, "{e}");
    assert!(e["error"].as_str().unwrap().contains("old side"));
    let (st, m) = http_post(
        &url,
        &json!({"revision": 0, "file": "/COMMIT_MSG", "line": 1, "body": "subject nit"}),
    );
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["line_text"], "core: x");
}

/// An old-side draft snapshots the parent tree; a new-side draft the commit's.
#[test]
fn old_side_draft_snapshots_parent_tree() {
    let g = GitRepo::new();
    let seed = g.commit(&[g.root], "seed\n", &[("f.txt", "old1\nold2\nold3\n")]);
    g.branch("main", seed);
    let c1 = g.commit(
        &[seed],
        &msg("edit f", "Ie"),
        &[("f.txt", "old1\nNEW2\nold3\n")],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ie");
    let url = drafts_url(&server, id);

    let (st, old) = http_post(
        &url,
        &json!({"revision": 0, "file": "f.txt", "line": 2, "side": "old", "body": "was?"}),
    );
    assert_eq!(st, 200, "{old}");
    assert_eq!(old["side"], "old");
    assert_eq!(old["line_text"], "old2", "old side reads the parent tree");

    let (_, new) = http_post(
        &url,
        &json!({"revision": 0, "file": "f.txt", "line": 2, "side": "new", "body": "now"}),
    );
    assert_eq!(new["line_text"], "NEW2", "new side reads the commit tree");
}

// ---------------------------------------------------------------------------
// Draft editing

/// PATCH rewrites a draft's body/resolution; DELETE removes it (404 after).
#[test]
fn patch_and_delete_draft() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ix");
    let url = drafts_url(&server, id);

    let (_, d) = http_post(
        &url,
        &json!({"revision": 0, "file": "x.txt", "line": 1, "body": "first"}),
    );
    let draft_id = d["id"].as_u64().unwrap();

    let (st, edited) = http_patch(
        &server.url(&format!("/api/drafts/{draft_id}")),
        &json!({"body": "second", "resolved": true}),
    );
    assert_eq!(st, 200, "{edited}");
    assert_eq!(edited["body"], "second");
    assert_eq!(edited["resolved"], true);
    // The change reflects the edited draft.
    let d = detail(&server, id);
    assert_eq!(d["drafts"][0]["body"], "second");

    let (st, _) = http_delete(&server.url(&format!("/api/drafts/{draft_id}")));
    assert_eq!(st, 204);
    let d = detail(&server, id);
    assert!(d["drafts"].as_array().unwrap().is_empty(), "draft deleted");

    // A second delete (and a patch) on the gone row is a 404.
    let (st, _) = http_delete(&server.url(&format!("/api/drafts/{draft_id}")));
    assert_eq!(st, 404);
    let (st, _) = http_patch(
        &server.url(&format!("/api/drafts/{draft_id}")),
        &json!({"body": "x"}),
    );
    assert_eq!(st, 404);
}

// ---------------------------------------------------------------------------
// Thread resolution (drafted, applied in draft order on publish)

/// Resolution is staged on a draft and applied on publish; an empty-body
/// resolution-only draft moves the thread without adding a comment.
#[test]
fn drafted_thread_resolution() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ix");
    let url = drafts_url(&server, id);

    // Publish a thread.
    let (_, _) = http_post(
        &url,
        &json!({"revision": 0, "file": "x.txt", "line": 1, "body": "why?"}),
    );
    review(&server, id, "comment", "");
    let thread_id = detail(&server, id)["threads"][0]["id"].as_u64().unwrap();

    let is_resolved = |server: &TestServer| -> bool {
        let t = thread_of(server, id, thread_id);
        // No empty-body resolution draft ever materializes as a comment.
        assert!(
            t["comments"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["body"].as_str() != Some("")),
            "empty resolution draft must not add a comment"
        );
        t["resolved"].as_bool().unwrap()
    };
    assert!(!is_resolved(&server), "new thread starts unresolved");

    // Resolve: an empty-body reply draft staging resolved=true, then publish.
    let (st, res_draft) = http_post(
        &url,
        &json!({"revision": 0, "thread_id": thread_id, "body": "", "resolved": true}),
    );
    assert_eq!(st, 200, "{res_draft}");
    assert_eq!(res_draft["resolved"], true);
    review(&server, id, "comment", "");
    assert!(is_resolved(&server), "drafted resolve applied on publish");

    // Reopen: stage resolved=false, then publish.
    let (st, _) = http_post(
        &url,
        &json!({"revision": 0, "thread_id": thread_id, "body": "", "resolved": false}),
    );
    assert_eq!(st, 200);
    review(&server, id, "comment", "");
    assert!(!is_resolved(&server), "drafted reopen applied on publish");
}

/// Drafts staging conflicting resolutions on one thread apply in draft order —
/// the thread ends at the last decision.
#[test]
fn resolution_applied_in_draft_order() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ix");
    let url = drafts_url(&server, id);

    let (_, _) = http_post(
        &url,
        &json!({"revision": 0, "file": "x.txt", "line": 1, "body": "q"}),
    );
    review(&server, id, "comment", "");
    let thread_id = detail(&server, id)["threads"][0]["id"].as_u64().unwrap();

    // Two resolution drafts on the same thread, last says reopen (false).
    let (st, _) = http_post(
        &url,
        &json!({"revision": 0, "thread_id": thread_id, "body": "", "resolved": true}),
    );
    assert_eq!(st, 200);
    let (st, _) = http_post(
        &url,
        &json!({"revision": 0, "thread_id": thread_id, "body": "still unsure", "resolved": false}),
    );
    assert_eq!(st, 200);
    review(&server, id, "comment", "");

    let t = thread_of(&server, id, thread_id);
    assert_eq!(t["resolved"], false, "the last drafted decision wins");
    // Only the non-empty draft added a comment.
    assert_eq!(t["comments"].as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Per-(change, revision) status: a pure rebase carries the verdict forward

/// A pure rebase (same patch-id + message, new parent) appends a revision but
/// carries the verdict forward; reviewing the new revision still works because
/// a live tip pins it.
#[test]
fn pure_rebase_carries_status_forward() {
    let g = GitRepo::new();
    let a_txt = "a1\na2\na3\n";
    let c0 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", a_txt)]);
    g.branch("feat", c0);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    review(&server, id, "approve", "lgtm");

    // Move main, re-parent the same patch onto it: same content, new sha.
    let m1 = g.commit(&[g.root], "mainline: unrelated\n", &[("b.txt", "b\n")]);
    g.branch("main", m1);
    let c1 = g.commit(&[m1], &msg("core: a", "Ia"), &[("a.txt", a_txt)]);
    g.branch("feat", c1);
    let id2 = push_one(&server, &g, "feat", "Ia");
    assert_eq!(id2, id);

    let d = detail(&server, id);
    assert_eq!(
        d["revisions"].as_array().unwrap().len(),
        2,
        "a rebase appends a revision"
    );
    // The pure rebase keeps the approve at rev 1 (status carried forward).
    let (_, chain) = http_get(&server.url(&format!("/api/chains/{id}")));
    let member = chain["path"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["change_id"].as_u64() == Some(id))
        .unwrap();
    assert_eq!(member["revision"], 1);
    assert_eq!(
        member["status"], "approved",
        "a pure rebase preserves the verdict"
    );
}

// ---------------------------------------------------------------------------
// Agent comments (never change review status)

/// The agent comment endpoint opens a thread / replies (`review_id` null) and never
/// moves the change's review status.
#[test]
fn agent_comment_opens_thread_without_review_status() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x1\nx2\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ix");
    let comments_url = server.url(&format!("/api/changes/{id}/comments"));

    // A new agent thread (published immediately; review_id null → agent).
    let (st, thread) = http_post(
        &comments_url,
        &json!({"revision": 0, "file": "x.txt", "line": 1, "body": "chose x1 deliberately"}),
    );
    assert_eq!(st, 200, "{thread}");
    let thread_id = thread["id"].as_u64().unwrap();
    assert!(thread["comments"][0]["review_id"].is_null());
    assert_eq!(thread["comments"][0]["body"], "chose x1 deliberately");
    assert_eq!(
        thread["comments"][0]["review_id"],
        Value::Null,
        "agent comment has no review"
    );

    // Status is unchanged by an agent comment (still pending, no review).
    let d = detail(&server, id);
    assert!(
        d["reviews"].as_array().unwrap().is_empty(),
        "no review created"
    );
    let (_, chain) = http_get(&server.url(&format!("/api/chains/{id}")));
    let member = chain["path"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["change_id"].as_u64() == Some(id))
        .unwrap();
    assert_eq!(member["status"], "pending");

    // A reply on the same thread (anchor fields ignored — the thread owns it).
    let (st, replied) = http_post(
        &comments_url,
        &json!({"thread_id": thread_id, "body": "and here is why"}),
    );
    assert_eq!(st, 200, "{replied}");
    let comments = replied["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert!(comments.iter().all(|c| c["review_id"].is_null()));

    // An empty-body agent comment with no resolution is a 400.
    let (st, _) = http_post(&comments_url, &json!({"revision": 0, "body": ""}));
    assert_eq!(st, 400, "empty agent comment rejected");
    // Replying to a non-existent thread is a 400.
    let (st, _) = http_post(&comments_url, &json!({"thread_id": 9999, "body": "hi"}));
    assert_eq!(st, 400, "reply to unknown thread rejected");
}

/// The reviewer engages an agent-initiated thread exactly like any other:
/// reply and resolve through drafts, applied on publish.
#[test]
fn reviewer_replies_to_agent_thread() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: x", "Ix"), &[("x.txt", "x1\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ix");

    // The agent opens a thread on its own change.
    let (st, thread) = http_post(
        &server.url(&format!("/api/changes/{id}/comments")),
        &json!({"revision": 0, "file": "x.txt", "line": 1, "body": "note: intentional"}),
    );
    assert_eq!(st, 200, "{thread}");
    let thread_id = thread["id"].as_u64().unwrap();

    // The reviewer replies + resolves via a draft, then publishes.
    let (st, _) = http_post(
        &drafts_url(&server, id),
        &json!({"revision": 0, "thread_id": thread_id, "body": "ack, thanks", "resolved": true}),
    );
    assert_eq!(st, 200);
    review(&server, id, "comment", "");

    let t = thread_of(&server, id, thread_id);
    assert_eq!(t["resolved"], true);
    let comments = t["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert!(comments[0]["review_id"].is_null());
    assert!(!comments[1]["review_id"].is_null());
    assert_eq!(comments[1]["body"], "ack, thanks");
}
