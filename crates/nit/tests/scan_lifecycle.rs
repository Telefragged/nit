//! Change lifecycle: merged detection when a change lands on the canonical
//! branch (the background timer's only job, prefix-merge included), plus
//! the explicit `abandon`/`reopen` actions and the 409-then-200 push gate
//! around an abandoned change.
//!
//! `merged` is written only by the background sweep, so the merged tests drive
//! one sweep synchronously through `sweep()` and assert. Abandonment is an
//! explicit action, not a sweep — those tests drive `POST .../abandon`
//! directly.

mod common;

use common::{
    GitRepo, TestServer, abandon, first_repo_id, http_get, http_post, member_id, msg, push, review,
    status_at, sweep, tip_change,
};
use serde_json::json;

#[test]
fn change_landed_on_main_becomes_merged() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&res, "I001");
    assert_eq!(tip_change(&res)["revision"], 0);
    assert_eq!(tip_change(&res)["status"], "pending");

    // Land the same change on the canonical ref: the timer recognises
    // its Change-Id.
    let merged = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("main", merged);

    sweep(&server);
    assert_eq!(
        status_at(&server, change_number, Some(0)).as_deref(),
        Some("merged")
    );

    let repo = first_repo_id(&server);
    let (_, pending) = http_get(&server.url(&format!("/api/changes?repo={repo}&status=pending")));
    assert!(
        pending["changes"].as_array().unwrap().is_empty(),
        "the merged change left the pending list: {pending}"
    );
    let (_, merged) = http_get(&server.url(&format!("/api/changes?repo={repo}&status=merged")));
    assert_eq!(merged["changes"][0]["id"], change_number);
}

#[test]
fn prefix_merge_marks_ancestor_while_tip_stays_live() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let tip = tip_change(&res)["change_number"].as_u64().unwrap();
    let ancestor = member_id(&res, "I001");
    assert_eq!(tip, member_id(&res, "I002"));

    // Land only the ancestor (I001) on main — the tip (I002) stays unlanded.
    let merged = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("main", merged);

    sweep(&server);
    assert_eq!(
        status_at(&server, ancestor, Some(0)).as_deref(),
        Some("merged")
    );
    assert_eq!(status_at(&server, tip, Some(0)).as_deref(), Some("pending"));
}

#[test]
fn branchless_change_stays_live_without_auto_abandon() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&res, "I001");

    // Delete the only branch, then move main with an unrelated commit (a
    // foreign Change-Id, so no false merge) so the sweep does real work
    // over the open set containing this change — and demonstrably leaves it
    // pending, never auto-abandoned.
    g.delete_branch("feat");
    let other = g.commit(&[g.root], &msg("unrelated", "I999"), &[("z.txt", "z\n")]);
    g.branch("main", other);
    sweep(&server);
    assert_eq!(
        status_at(&server, change_number, Some(0)).as_deref(),
        Some("pending"),
        "a branch-less change stays live"
    );
}

#[test]
fn reopen_clears_abandoned_to_retained_status() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&res, "I001");

    // Approve, then abandon: the verdict is retained, masked by the overlay.
    review(&server, change_number, "approve", "lgtm");
    abandon(&server, change_number);

    let (st, detail) = http_post(
        &server.url(&format!("/api/changes/{change_number}/reopen")),
        &json!({}),
    );
    assert_eq!(st, 200, "{detail}");
    assert_eq!(detail["id"], change_number);
    assert_eq!(
        status_at(&server, change_number, Some(0)).as_deref(),
        Some("approved"),
        "reopen surfaces the retained verdict"
    );
}

#[test]
fn push_to_merged_change_409s() {
    // A Change-Id is never reused: without the gate, a new revision would
    // paint the merged overlay onto unreviewed content.
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");

    let merged = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("main", merged);
    sweep(&server);

    let c1b = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "different\n")]);
    g.branch("feat", c1b);
    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 409, "{e}");
    assert!(e["error"].as_str().unwrap().contains("merged"), "{e}");
}

#[test]
fn push_to_abandoned_change_409s_until_reopened() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&res, "I001");

    abandon(&server, change_number);
    let c1b = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "different\n")]);
    g.branch("feat", c1b);

    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 409, "{e}");
    assert!(e["error"].as_str().unwrap().contains("abandoned"), "{e}");

    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_number}/reopen")),
        &json!({}),
    );
    assert_eq!(st, 200);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    assert_eq!(tip_change(&res)["revision"], 1, "the new revision merged");
    assert_eq!(
        tip_change(&res)["status"],
        "pending",
        "a content change resets status"
    );
}

#[test]
fn re_push_of_unchanged_abandoned_revision_is_not_blocked() {
    // The 409 guards a revision that *moves*; an idempotent re-push of the
    // already-recorded sha must not trip it.
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&res, "I001");

    // The branch still points at the same sha — abandon doesn't move it.
    abandon(&server, change_number);

    // Re-pushing the same sha walks to nothing that moves, so the 409 guard
    // (which fires only on a moving revision) never trips — idempotent 200.
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    assert_eq!(tip_change(&res)["revision"], 0, "no new revision recorded");
}
