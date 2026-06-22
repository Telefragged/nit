//! Staged reviewer decisions + the per-chain batch submit (docs/api.md
//! "Reviewer decisions", "Chains"). A decision is reviewer scratch like a
//! comment draft (`PUT`/`DELETE /api/changes/{id}/decision`), published only by
//! `POST /api/chains/{id}/submit`, which publishes each member's staged decision
//! at the revision the chain path pins. Abandonment is a decision, not a
//! separate button; submit is idempotent (a published decision's row is gone).

mod common;

use common::*;
use serde_json::{Value, json};

fn push_one(server: &TestServer, g: &GitRepo, tip: &str, change_key: &str) -> u64 {
    let (st, res) = push(server, g, tip, "main", None);
    assert_eq!(st, 200, "{res}");
    member_id(&res, change_key)
}

fn detail(server: &TestServer, change_id: u64) -> Value {
    let (st, d) = http_get(&server.url(&format!("/api/changes/{change_id}")));
    assert_eq!(st, 200, "{d}");
    d
}

fn stage(server: &TestServer, change_id: u64, decision: &str, message: &str) {
    let (st, d) = http_put(
        &server.url(&format!("/api/changes/{change_id}/decision")),
        &json!({"decision": decision, "message": message}),
    );
    assert_eq!(st, 200, "{d}");
    assert_eq!(d["decision"], decision);
}

fn submit_chain(server: &TestServer, tip: u64) -> Value {
    let (st, out) = http_post(
        &server.url(&format!("/api/chains/{tip}/submit")),
        &json!({}),
    );
    assert_eq!(st, 200, "{out}");
    out
}

/// A path member's `status` off the change's derived chain (the change is its
/// own tip for a single-commit chain).
fn status_at(server: &TestServer, change_id: u64) -> String {
    let (_, chain) = http_get(&server.url(&format!("/api/chains/{change_id}")));
    chain["path"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["change_id"].as_u64() == Some(change_id))
        .and_then(|m| m["status"].as_str())
        .unwrap_or("?")
        .to_string()
}

fn draft_comment(server: &TestServer, change_id: u64, file: &str, line: u64, body: &str) {
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/drafts")),
        &json!({"revision": 0, "file": file, "line": line, "body": body}),
    );
    assert_eq!(st, 200);
}

// ---------------------------------------------------------------------------
// Stage / clear

/// A staged decision surfaces on both the change detail (with its message) and
/// the chain path member (change-wide); clearing it removes it.
#[test]
fn stage_surfaces_then_clears() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    assert_eq!(detail(&server, id)["draft_decision"], Value::Null);

    stage(&server, id, "approve", "lgtm");
    let d = detail(&server, id);
    assert_eq!(d["draft_decision"]["decision"], "approve");
    assert_eq!(d["draft_decision"]["message"], "lgtm");

    // Staging again overwrites.
    stage(&server, id, "request_changes", "actually, no");
    assert_eq!(
        detail(&server, id)["draft_decision"]["decision"],
        "request_changes"
    );

    // Clear it.
    let (st, _) = http_delete(&server.url(&format!("/api/changes/{id}/decision")));
    assert_eq!(st, 204);
    assert_eq!(detail(&server, id)["draft_decision"], Value::Null);
    // A second clear is still 204 (a no-op).
    let (st, _) = http_delete(&server.url(&format!("/api/changes/{id}/decision")));
    assert_eq!(st, 204);

    // Staging is not a publish: no review, status still pending.
    assert!(
        detail(&server, id)["reviews"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(status_at(&server, id), "pending");
}

// ---------------------------------------------------------------------------
// Batch submit

/// Batch submit publishes a staged verdict, draining the change's comment
/// drafts into the review and clearing the staged decision.
#[test]
fn batch_submit_publishes_verdict_and_drains_comments() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a1\na2\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    draft_comment(&server, id, "a.txt", 2, "why a2?");
    stage(&server, id, "request_changes", "a nit");

    let out = submit_chain(&server, id);
    assert_eq!(out["submitted"], 1);
    assert!(out["errors"].as_array().unwrap().is_empty());

    let d = detail(&server, id);
    assert_eq!(d["draft_decision"], Value::Null, "decision cleared");
    assert!(
        d["drafts"].as_array().unwrap().is_empty(),
        "comments drained"
    );
    assert_eq!(d["reviews"].as_array().unwrap().len(), 1);
    assert_eq!(d["reviews"][0]["verdict"], "request_changes");
    assert_eq!(d["reviews"][0]["message"], "a nit");
    assert_eq!(
        d["threads"].as_array().unwrap().len(),
        1,
        "comment published"
    );
    assert_eq!(status_at(&server, id), "changes_requested");
}

/// A change with comment drafts but NO staged decision is left untouched by
/// batch submit — comments never auto-publish (they would flip an approved
/// change to commented). They stay drafts until the reviewer decides.
#[test]
fn batch_submit_leaves_undecided_comment_only_change() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    draft_comment(&server, id, "a.txt", 1, "a note, no verdict");

    let out = submit_chain(&server, id);
    assert_eq!(out["submitted"], 0, "nothing staged → nothing published");
    let d = detail(&server, id);
    assert_eq!(
        d["drafts"].as_array().unwrap().len(),
        1,
        "comment kept as draft"
    );
    assert!(d["reviews"].as_array().unwrap().is_empty());
    assert_eq!(status_at(&server, id), "pending");
}

/// Abandonment is a decision: a staged `abandon` publishes a `lifecycle`
/// abandoned (with its message as the reason) on batch submit, and still drains
/// any comment drafts so staged work is never stranded.
#[test]
fn batch_submit_abandon_decision_drains_and_records_reason() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    draft_comment(&server, id, "a.txt", 1, "this is why it is wrong");
    stage(&server, id, "abandon", "superseded by another approach");

    let out = submit_chain(&server, id);
    assert_eq!(out["submitted"], 1);
    assert_eq!(status_at(&server, id), "abandoned");

    let d = detail(&server, id);
    assert_eq!(d["draft_decision"], Value::Null);
    assert!(
        d["drafts"].as_array().unwrap().is_empty(),
        "comment drained"
    );
    assert_eq!(
        d["threads"].as_array().unwrap().len(),
        1,
        "comment published"
    );

    // The reason rides the lifecycle{abandoned} entry.
    let (_, log) = http_get(&server.url(&format!("/api/chains/{id}/log")));
    let abandoned = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "lifecycle" && e["payload"]["action"] == "abandoned")
        .expect("a lifecycle{abandoned} entry");
    assert_eq!(
        abandoned["payload"]["message"],
        "superseded by another approach"
    );
}

/// A staged `reopen` on an abandoned change clears it back to live on submit.
#[test]
fn batch_submit_reopen_decision() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    // Abandon it (the immediate endpoint), then stage + submit a reopen.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{id}/abandon")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(status_at(&server, id), "abandoned");

    stage(&server, id, "reopen", "");
    let out = submit_chain(&server, id);
    assert_eq!(out["submitted"], 1);
    assert_eq!(status_at(&server, id), "pending", "reopened back to live");
    assert_eq!(detail(&server, id)["draft_decision"], Value::Null);
}

/// A decision illegal for the member's current lifecycle (a verdict on an
/// abandoned change) is skipped into `errors` and its row is kept.
#[test]
fn batch_submit_skips_illegal_decision_keeps_row() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{id}/abandon")),
        &json!({}),
    );
    assert_eq!(st, 200);
    stage(&server, id, "approve", "lgtm"); // a verdict on an abandoned change

    let out = submit_chain(&server, id);
    assert_eq!(out["submitted"], 0);
    let errors = out["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["change_id"], id);
    assert!(errors[0]["message"].as_str().unwrap().contains("abandoned"));
    // The staged decision is kept so the reviewer can fix it.
    assert_eq!(detail(&server, id)["draft_decision"]["decision"], "approve");
}

/// Submit is idempotent: re-submitting after a decision published is a no-op
/// (its row is gone), so a torn batch is finished by re-clicking submit.
#[test]
fn batch_submit_is_idempotent() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    stage(&server, id, "approve", "lgtm");
    assert_eq!(submit_chain(&server, id)["submitted"], 1);
    // Re-submit: the row is gone, so nothing republishes.
    assert_eq!(submit_chain(&server, id)["submitted"], 0);
    assert_eq!(
        detail(&server, id)["reviews"].as_array().unwrap().len(),
        1,
        "no double review"
    );
    assert_eq!(status_at(&server, id), "approved");
}

/// One submit publishes every member's staged decision, each at the revision
/// the chain path pins on it.
#[test]
fn batch_submit_publishes_every_member() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    let b = g.commit(&[a], &msg("core: b", "Ib"), &[("b.txt", "b\n")]);
    g.branch("feat", b);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{res}");
    let id_a = member_id(&res, "Ia");
    let id_b = member_id(&res, "Ib");

    stage(&server, id_a, "approve", "a lgtm");
    stage(&server, id_b, "request_changes", "b needs work");

    let out = submit_chain(&server, id_b); // tip is B; the path is A → B
    assert_eq!(out["submitted"], 2);
    assert!(out["errors"].as_array().unwrap().is_empty());
    assert_eq!(status_at(&server, id_a), "approved");
    assert_eq!(status_at(&server, id_b), "changes_requested");
}

/// A decision publishes at the revision the chain pins (the live latest), never
/// a superseded patchset: after an amend, submitting approves r1, leaving r0's
/// own status untouched.
#[test]
fn batch_submit_publishes_at_pinned_revision() {
    let g = GitRepo::new();
    let c0 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a1\n")]);
    g.branch("feat", c0);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    // Amend (content change → r1); the chain now pins r1.
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a1\na2\n")]);
    g.branch("feat", c1);
    let id2 = push_one(&server, &g, "feat", "Ia");
    assert_eq!(id2, id);

    stage(&server, id, "approve", "lgtm");
    assert_eq!(submit_chain(&server, id)["submitted"], 1);

    let d = detail(&server, id);
    let review = &d["reviews"][0];
    assert_eq!(
        review["revision"], 1,
        "published at the pinned (latest) revision"
    );
    // r1 shows approved; r0 carries no review of its own.
    let (_, chain) = http_get(&server.url(&format!("/api/chains/{id}?revision=1")));
    assert_eq!(chain["path"][0]["status"], "approved");
    let (_, r0) = http_get(&server.url(&format!("/api/chains/{id}?revision=0")));
    assert_eq!(r0["path"][0]["status"], "pending");
}

// ---------------------------------------------------------------------------
// Validation

/// An unknown decision value is a 400 (enum deserialize); an unknown change is
/// a 404.
#[test]
fn stage_validation() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let id = push_one(&server, &g, "feat", "Ia");

    let (st, _) = http_put(
        &server.url(&format!("/api/changes/{id}/decision")),
        &json!({"decision": "maybe", "message": ""}),
    );
    assert_eq!(st, 400, "bad decision value");

    let (st, _) = http_put(
        &server.url("/api/changes/99999/decision"),
        &json!({"decision": "approve"}),
    );
    assert_eq!(st, 404, "unknown change");
}
