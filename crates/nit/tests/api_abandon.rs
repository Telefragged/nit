//! The abandon action (`POST /api/changes/{id}/abandon`, `nit abandon`): an
//! explicit reviewer or author judgment that a change is dead, reversible by
//! reopen. Distinct from the background timer — the change here stays reachable
//! from a branch, so only the explicit action abandons it.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, member_id, msg, push, status_at};
use serde_json::json;

#[test]
fn abandon_action_marks_the_change_abandoned_and_records_a_reason() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    let change_number = member_id(&server, &res, "I001");
    assert_eq!(
        status_at(&server, change_number, Some(0)).as_deref(),
        Some("pending")
    );

    let (st, detail) = http_post(
        &server.url(&format!("/api/changes/{change_number}/abandon")),
        &json!({"message": "superseded by another approach"}),
    );
    assert_eq!(st, 200, "{detail}");
    assert_eq!(
        status_at(&server, change_number, Some(0)).as_deref(),
        Some("abandoned")
    );

    let (_, log) = http_get(&server.url(&format!("/api/chains/{change_number}/log")));
    let abandoned = log["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|e| e["kind"] == "lifecycle" && e["payload"]["action"] == "abandoned")
        .expect("a lifecycle{abandoned} entry");
    assert_eq!(
        abandoned["payload"]["message"], "superseded by another approach",
        "the reason is stored: {abandoned}"
    );

    // Idempotent: re-abandoning an already-abandoned change is a no-op.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_number}/abandon")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(
        status_at(&server, change_number, Some(0)).as_deref(),
        Some("abandoned")
    );

    // Reopen clears it back to the retained (pending) status.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_number}/reopen")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(
        status_at(&server, change_number, Some(0)).as_deref(),
        Some("pending")
    );
}
