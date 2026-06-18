//! The abandon action (`POST /api/changes/{id}/abandon`, `nit abandon`): an
//! explicit reviewer/agent judgment that a change is dead, reversible by
//! reopen. Distinct from the background timer — the change here stays reachable
//! from a branch, so only the explicit action abandons it.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, member_id, msg, push};
use serde_json::json;

/// Per-revision status of `change_id` off its derived chain path (the change is
/// its own degenerate tip once terminal).
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
fn abandon_action_marks_the_change_abandoned_and_records_a_reason() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{res}");
    let change_id = member_id(&res, "I001");
    assert_eq!(status_at(&server, change_id, 0).as_deref(), Some("pending"));

    // Explicit abandon with a reason.
    let (st, detail) = http_post(
        &server.url(&format!("/api/changes/{change_id}/abandon")),
        &json!({"message": "superseded by another approach"}),
    );
    assert_eq!(st, 200, "{detail}");
    assert_eq!(
        status_at(&server, change_id, 0).as_deref(),
        Some("abandoned")
    );

    // The reason is recorded on the lifecycle entry.
    let (_, log) = http_get(&server.url(&format!("/api/changes/{change_id}/log")));
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

    // Idempotent: abandoning again (bodyless) is a no-op, still abandoned.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/abandon")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(
        status_at(&server, change_id, 0).as_deref(),
        Some("abandoned")
    );

    // Reopen clears it back to the retained (pending) status.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reopen")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(status_at(&server, change_id, 0).as_deref(), Some("pending"));
}
