use super::*;
use crate::enums::ChangeStatus;

fn change_row() -> db::ChangeRow {
    db::ChangeRow {
        id: 1,
        repo_id: 1,
        change_key: "Iabc".to_string(),
        created_at: "t0".to_string(),
    }
}

fn entry(idx: u64, kind: &str, payload: serde_json::Value) -> Entry {
    Entry {
        seq: idx,
        idx,
        kind: kind.parse().expect("test log kind"),
        payload,
        created_at: format!("t{idx}"),
    }
}

/// Apply a revision entry; the fold mints its 0-based number.
fn revision(sha: &str, parent: &str, base: &str, resets: bool) -> serde_json::Value {
    serde_json::json!({
        "commit_sha": sha, "parent_sha": parent, "base_sha": base,
        "message": format!("subject {sha}\n\nChange-Id: Iabc\n"),
        "partial": false, "resets_status": resets,
    })
}

fn review(revision: u64, verdict: &str) -> serde_json::Value {
    serde_json::json!({
        "review_id": 100 + revision, "revision": revision,
        "verdict": verdict, "message": "msg", "comments": [],
    })
}

/// Fold a sequence of (kind, payload) onto a fresh change, idx-ordered.
fn folded(entries: &[(&str, serde_json::Value)]) -> ChangeProj {
    let mut c = ChangeProj::empty(&change_row());
    for (i, (kind, payload)) in entries.iter().enumerate() {
        let e = entry(
            u64::try_from(i).expect("index fits u64"),
            kind,
            payload.clone(),
        );
        fold(&mut c, &e).expect("fold");
    }
    c
}

#[test]
fn revisions_are_zero_based_and_minted_in_the_fold() {
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("revision", revision("B", "A", "base", true)),
    ]);
    assert_eq!(c.revisions.len(), 2);
    assert_eq!(c.revisions[0].number, 0);
    assert_eq!(c.revisions[1].number, 1);
    assert_eq!(c.latest_revision().expect("a revision").commit_sha, "B");
}

#[test]
fn status_is_per_revision() {
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "request_changes")),
        ("revision", revision("B", "base", "base", true)), // reword: new patchset
    ]);
    // The review landed on r0; r1 has no review yet and resets → pending.
    assert_eq!(c.status_at(0), ChangeStatus::ChangesRequested);
    assert_eq!(c.status_at(1), ChangeStatus::Pending);
}

#[test]
fn pure_rebase_carries_status_forward() {
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "approve")),
        // r1 is a pure rebase (resets_status = false): inherits r0's approve.
        ("revision", revision("B", "base2", "base2", false)),
    ]);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(c.status_at(1), ChangeStatus::Approved);
}

#[test]
fn reword_resets_status() {
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "approve")),
        // r1 is a reword (resets_status = true): back to pending.
        ("revision", revision("B", "base", "base", true)),
    ]);
    assert_eq!(c.status_at(1), ChangeStatus::Pending);
}

#[test]
fn merged_is_per_revision() {
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "approve")),
        ("revision", revision("B", "base", "base", true)),
        (
            "lifecycle",
            serde_json::json!({"action": "merged", "revision": 1}),
        ),
    ]);
    // Only the landed revision shows merged; older ones show their status +
    // merged_elsewhere.
    assert_eq!(c.status_at(1), ChangeStatus::Merged);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert!(c.merged_elsewhere(0));
    assert!(!c.merged_elsewhere(1));
    assert!(c.is_terminal());
}

#[test]
fn abandon_then_reopen() {
    let mut c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "request_changes")),
        ("lifecycle", serde_json::json!({"action": "abandoned"})),
    ]);
    assert_eq!(c.status_at(0), ChangeStatus::Abandoned);
    assert!(c.is_terminal());
    // Reopen restores the retained verdict status.
    let e = entry(
        c.head,
        "lifecycle",
        serde_json::json!({"action": "reopened"}),
    );
    fold(&mut c, &e).expect("fold");
    assert!(!c.is_terminal());
    assert_eq!(c.status_at(0), ChangeStatus::ChangesRequested);
}

#[test]
fn partial_flag_restamps_the_latest_revision() {
    let mut c = folded(&[("revision", revision("A", "base", "base", true))]);
    assert!(!c.is_partial());
    let e = entry(c.head, "partial", serde_json::json!({"partial": true}));
    fold(&mut c, &e).expect("fold");
    assert!(c.is_partial());
}

#[test]
fn threads_open_reply_and_resolve() {
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        // A review opening a thread (reviewer), then a resolving reply.
        (
            "review",
            serde_json::json!({
                "review_id": 200, "revision": 0, "verdict": "comment", "message": "",
                "comments": [{"revision": 0, "file": "src/x.rs", "line": 3, "side": "new", "body": "look"}],
            }),
        ),
        (
            "review",
            serde_json::json!({
                "review_id": 201, "revision": 0, "verdict": "approve", "message": "",
                "comments": [{"thread_id": 0, "body": "fixed", "resolved": true}],
            }),
        ),
    ]);
    assert_eq!(c.threads.len(), 1);
    assert_eq!(c.threads[0].comments.len(), 2);
    assert!(c.threads[0].resolved);
    assert_eq!(c.unresolved_at(0), 0);
}

#[test]
fn agent_comment_opens_a_thread() {
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        (
            "comment",
            serde_json::json!({"revision": 0, "file": "a.rs", "line": 1, "side": "new", "body": "why?"}),
        ),
    ]);
    assert_eq!(c.threads.len(), 1);
    assert_eq!(c.threads[0].comments[0].author, crate::enums::Author::Agent);
    assert_eq!(c.unresolved_at(0), 1);
}

#[test]
fn replay_round_trips() {
    let rows: Vec<db::LogRow> = [
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "approve")),
    ]
    .iter()
    .enumerate()
    .map(|(i, (kind, payload))| db::LogRow {
        seq: u64::try_from(i).expect("index fits u64"),
        idx: u64::try_from(i).expect("index fits u64"),
        kind: (*kind).to_string(),
        payload: payload.to_string(),
        created_at: format!("t{i}"),
    })
    .collect();
    let c = replay(&change_row(), &rows).expect("replay");
    assert_eq!(c.revisions.len(), 1);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(max_assigned_id(&rows).expect("max"), 100);
}
