//! Pure-rebase vs reword semantics through `POST /api/push`: a patch-id-equal,
//! same-message commit on a new parent appends a revision but carries the
//! reviewed status forward; changing the message resets the change to pending.
//! Revisions are 0-based (rev 0 is the first). Asserted through
//! `GET /api/changes/{id}` and the derived chain path.

mod common;

use common::{GitRepo, TestServer, http_get, member_id, msg, push, review};
use serde_json::Value;

fn change_detail(server: &TestServer, change_id: u64) -> Value {
    let (st, v) = http_get(&server.url(&format!("/api/changes/{change_id}")));
    assert_eq!(st, 200, "{v}");
    v
}

fn path_status(server: &TestServer, tip_change_id: u64, change_key: &str) -> String {
    let (st, chain) = http_get(&server.url(&format!("/api/chains/{tip_change_id}")));
    assert_eq!(st, 200, "{chain}");
    chain["path"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["change_key"] == change_key)
        .unwrap_or_else(|| panic!("no {change_key} in path: {chain}"))["status"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Approve a change at its live pinned revision.
fn approve(server: &TestServer, change_id: u64) {
    review(server, change_id, "approve", "lgtm");
}

/// A pure rebase (same patch-id + same message, new parent) appends a revision
/// but the displayed status at the pinned revision carries the approval
/// forward; a reword (message changed) resets that change to pending.
#[test]
fn pure_rebase_carries_status_forward_then_reword_resets() {
    let g = GitRepo::new();
    let a_txt = "a1\na2\na3\n";
    let b_txt = "b1\nb2\n";
    let c1 = g.commit(&[g.root], &msg("one", "Ia"), &[("a.txt", a_txt)]);
    let c2 = g.commit(&[c1], &msg("two", "Ib"), &[("b.txt", b_txt)]);
    g.branch("feat", c2);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    let tip_id = member_id(&server, &pr, "Ib");
    assert_eq!(pr["tip_change"]["revision"], 0, "first revision is rev 0");

    approve(&server, tip_id);
    assert_eq!(path_status(&server, tip_id, "Ib"), "approved");

    let m1 = g.commit(&[g.root], "main: unrelated\n", &[("m.txt", "m\n")]);
    g.branch("main", m1);
    let c1r = g.commit(&[m1], &msg("one", "Ia"), &[("a.txt", a_txt)]);
    let c2r = g.commit(&[c1r], &msg("two", "Ib"), &[("b.txt", b_txt)]);
    g.branch("feat", c2r);

    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    assert_eq!(
        pr["tip_change"]["revision"], 1,
        "a pure rebase appends rev 1"
    );
    assert_eq!(
        pr["tip_change"]["status"], "approved",
        "the approval carries forward across a pure rebase"
    );
    assert_eq!(
        member_id(&server, &pr, "Ib"),
        tip_id,
        "same change identity"
    );

    let detail = change_detail(&server, tip_id);
    let revs = detail["revisions"].as_array().unwrap();
    assert_eq!(revs.len(), 2, "rev 0 and rev 1");
    assert_eq!(revs[1]["number"], 1);
    assert_eq!(revs[1]["commit_sha"], c2r.to_string());
    assert_eq!(revs[1]["parent_sha"], c1r.to_string());
    assert_eq!(
        detail["reviews"][0]["revision"], 0,
        "the verdict stays anchored to rev 0"
    );
    assert_eq!(path_status(&server, tip_id, "Ib"), "approved");

    // A reword changes reviewable content, so it resets to pending — a new
    // revision the reviewer hasn't seen.
    let c2w = g.commit(&[c1r], &msg("two: explained", "Ib"), &[("b.txt", b_txt)]);
    g.branch("feat", c2w);
    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    assert_eq!(pr["tip_change"]["revision"], 2, "the reword appends rev 2");
    assert_eq!(
        pr["tip_change"]["status"], "pending",
        "a reword resets the displayed status"
    );

    let detail = change_detail(&server, tip_id);
    assert_eq!(detail["revisions"].as_array().unwrap().len(), 3);
    assert_eq!(path_status(&server, tip_id, "Ib"), "pending");
}

/// A re-push where nothing moved is idempotent: no new revision, and the
/// carried status is unchanged.
#[test]
fn re_push_of_an_unchanged_tip_is_idempotent() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("only", "Ic"), &[("c.txt", "c\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    let change_id = member_id(&server, &pr, "Ic");
    approve(&server, change_id);

    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    assert_eq!(pr["tip_change"]["revision"], 0, "no new revision");
    assert_eq!(pr["tip_change"]["status"], "approved");

    let detail = change_detail(&server, change_id);
    assert_eq!(
        detail["revisions"].as_array().unwrap().len(),
        1,
        "a no-op re-push records nothing"
    );
}

/// A pure rebase carries `changes_requested` forward exactly like `approved`,
/// and the subsequent reword resets it — the reset is verdict-independent.
#[test]
fn pure_rebase_carries_request_changes_reword_resets() {
    let g = GitRepo::new();
    let txt = "x1\nx2\n";
    let c1 = g.commit(&[g.root], &msg("feat: x", "Ix"), &[("x.txt", txt)]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    let change_id = member_id(&server, &pr, "Ix");

    review(&server, change_id, "request_changes", "rename");
    assert_eq!(path_status(&server, change_id, "Ix"), "changes_requested");

    let m1 = g.commit(&[g.root], "main moves\n", &[("m.txt", "m\n")]);
    g.branch("main", m1);
    let c1r = g.commit(&[m1], &msg("feat: x", "Ix"), &[("x.txt", txt)]);
    g.branch("feat", c1r);
    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    assert_eq!(pr["tip_change"]["revision"], 1);
    assert_eq!(
        pr["tip_change"]["status"], "changes_requested",
        "request_changes carries forward across a pure rebase"
    );

    let c1w = g.commit(&[m1], &msg("feat: x, reworded", "Ix"), &[("x.txt", txt)]);
    g.branch("feat", c1w);
    let (st, pr) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pr}");
    assert_eq!(pr["tip_change"]["revision"], 2);
    assert_eq!(pr["tip_change"]["status"], "pending");
    assert_eq!(path_status(&server, change_id, "Ix"), "pending");
}
