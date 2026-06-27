//! Change lifecycle (docs/api.md "State table", docs/data-model.md
//! "Lifecycle"): merged detection when a change's patch lands on the canonical
//! branch (the background timer's only job, prefix-merge included), plus the
//! explicit `abandon`/`reopen` actions and the 409-then-200 push gate around an
//! abandoned change.
//!
//! `merged` is written only by the background sweep, so the merged tests drive
//! one sweep synchronously through `sweep()` and assert. Abandonment is an
//! explicit action, not a sweep — those tests drive `POST .../abandon`
//! directly.

mod common;

use common::{
    GitRepo, TestServer, first_repo_id, http_get, http_post, member_id, msg, push, review, sweep,
};
use serde_json::json;

/// The per-revision status of `change_id` at `revision`, read off the derived
/// chain path (the change is its own degenerate tip after it goes terminal).
fn status_at(server: &TestServer, change_id: u64, revision: u64) -> Option<String> {
    let (st, chain) =
        http_get(&server.url(&format!("/api/chains/{change_id}?revision={revision}")));
    if st != 200 {
        return None;
    }
    chain["path"]
        .as_array()?
        .iter()
        .find(|m| m["change_id"].as_u64() == Some(change_id))
        .and_then(|m| m["status"].as_str().map(str::to_string))
}

#[test]
fn change_landed_on_main_becomes_merged() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&server, &res, "I001");
    assert_eq!(res["tip_change"]["revision"], 0);
    assert_eq!(res["tip_change"]["status"], "pending");

    // Land the same change on the canonical branch: same Change-Id, same
    // patch-id (identical tree edit). The timer recognises the patch.
    let landed = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("main", landed);

    sweep(&server);
    assert_eq!(status_at(&server, change_id, 0).as_deref(), Some("merged"));

    // A fully-merged chain drops off the active dashboard but stays reachable
    // by id (and under ?status=all).
    let repo = first_repo_id(&server);
    let (_, active) = http_get(&server.url(&format!("/api/chains?repo={repo}&status=active")));
    assert!(
        active["chains"].as_array().unwrap().is_empty(),
        "merged chain left the active list: {active}"
    );
    let (_, all) = http_get(&server.url(&format!("/api/chains?repo={repo}&status=all")));
    assert_eq!(all["chains"][0]["state"], "merged");
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
    let tip = res["tip_change"]["change_id"].as_u64().unwrap();
    let ancestor = member_id(&server, &res, "I001");
    assert_eq!(tip, member_id(&server, &res, "I002"));

    // Land only the ancestor (I001) on main — the tip (I002) stays unlanded.
    let landed = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("main", landed);

    sweep(&server);
    assert_eq!(status_at(&server, ancestor, 0).as_deref(), Some("merged"));
    assert_eq!(status_at(&server, tip, 0).as_deref(), Some("pending"));

    // One live member keeps the partially-landed stack on the active list, but
    // the walk stops at the canonical branch: the ancestor has landed, so it
    // drops out of the path — only the open tip remains.
    let repo = first_repo_id(&server);
    let (_, active) = http_get(&server.url(&format!("/api/chains?repo={repo}&status=active")));
    let chains = active["chains"].as_array().unwrap();
    assert_eq!(chains.len(), 1, "stack stays visible: {active}");
    let path = chains[0]["path"].as_array().unwrap();
    assert_eq!(
        path.len(),
        1,
        "the merged ancestor drops from the path: {active}"
    );
    assert_eq!(path[0]["change_id"], tip);
    assert_eq!(path[0]["status"], "pending");
    assert!(
        path.iter().all(|m| m["change_id"] != ancestor),
        "the merged ancestor sits below the canonical branch now: {active}"
    );
}

fn abandon(server: &TestServer, change_id: u64) {
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/abandon")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(
        status_at(server, change_id, 0).as_deref(),
        Some("abandoned")
    );
}

#[test]
fn branchless_change_stays_live_without_auto_abandon() {
    // The core inversion: a change off every branch is NOT abandoned. Only the
    // explicit action abandons.
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&server, &res, "I001");

    // Delete the only branch, then move main with an unrelated commit (distinct
    // patch-id, so no false landing) so the sweep does real work over the open
    // set containing this change — and demonstrably leaves it pending, never
    // auto-abandoned.
    g.delete_branch("feat");
    let other = g.commit(&[g.root], &msg("unrelated", "I999"), &[("z.txt", "z\n")]);
    g.branch("main", other);
    sweep(&server);
    assert_eq!(
        status_at(&server, change_id, 0).as_deref(),
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
    let change_id = member_id(&server, &res, "I001");

    // Approve, then abandon: the verdict is retained, masked by the overlay.
    review(&server, change_id, "approve", "lgtm");
    abandon(&server, change_id);

    let (st, detail) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reopen")),
        &json!({}),
    );
    assert_eq!(st, 200, "{detail}");
    assert_eq!(detail["id"], change_id);
    assert_eq!(
        status_at(&server, change_id, 0).as_deref(),
        Some("approved"),
        "reopen surfaces the retained verdict"
    );
}

#[test]
fn push_to_abandoned_change_409s_until_reopened() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&server, &res, "I001");

    abandon(&server, change_id);
    let c1b = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "different\n")]);
    g.branch("feat", c1b);

    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 409, "{e}");
    assert!(e["error"].as_str().unwrap().contains("abandoned"), "{e}");

    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reopen")),
        &json!({}),
    );
    assert_eq!(st, 200);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["tip_change"]["revision"], 1, "the new revision landed");
    assert_eq!(
        res["tip_change"]["status"], "pending",
        "a content change resets status"
    );
}

#[test]
fn re_push_of_unchanged_abandoned_revision_is_not_blocked() {
    // The 409 guards a revision that *moves*; an idempotent re-push of the
    // already-recorded sha must not trip it (docs/api.md "Push").
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&server, &res, "I001");

    // The branch still points at the same sha — abandon doesn't move it.
    abandon(&server, change_id);

    // Re-pushing the same sha walks to nothing that moves, so the 409 guard
    // (which fires only on a moving revision) never trips — idempotent 200.
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["tip_change"]["revision"], 0, "no new revision recorded");
}
