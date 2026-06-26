use super::*;
use crate::enums::{ChangeStatus, LogKind};

fn change_row() -> db::ChangeRow {
    db::ChangeRow {
        id: 1,
        repo_id: 1,
        change_key: "Iabc".to_string(),
        status: None,
        created_at: "t0".to_string(),
    }
}

fn entry(idx: u64, kind: &str, payload: &serde_json::Value) -> Entry {
    let kind: LogKind = kind.parse().expect("test log kind");
    Entry {
        seq: idx,
        idx,
        payload: EntryPayload::from_json(kind, &payload.to_string()).expect("test payload"),
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
        let e = entry(u64::try_from(i).expect("index fits u64"), kind, payload);
        fold(&mut c, e);
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
fn current_status_tracks_the_latest_revision() {
    // No revisions yet: the cached value is pending.
    assert_eq!(
        ChangeProj::empty(&change_row()).current_status(),
        ChangeStatus::Pending
    );
    // current_status is the displayed status at the latest revision: r1 has no
    // review, so pending — even though r0 was approved.
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "approve")),
        ("revision", revision("B", "base", "base", true)),
    ]);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(c.current_status(), ChangeStatus::Pending);
    // A verdict on the latest revision moves the current status.
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "approve")),
    ]);
    assert_eq!(c.current_status(), ChangeStatus::Approved);
    // The lifecycle overlay wins change-wide: abandoned regardless of revision.
    let c = folded(&[
        ("revision", revision("A", "base", "base", true)),
        ("review", review(0, "approve")),
        ("lifecycle", serde_json::json!({"action": "abandoned"})),
    ]);
    assert_eq!(c.current_status(), ChangeStatus::Abandoned);
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
    // Only the landed revision shows merged; older ones show their own status.
    assert_eq!(c.status_at(1), ChangeStatus::Merged);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
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
    let e = entry(3, "lifecycle", &serde_json::json!({"action": "reopened"}));
    fold(&mut c, e);
    assert!(!c.is_terminal());
    assert_eq!(c.status_at(0), ChangeStatus::ChangesRequested);
}

#[test]
fn partial_flag_restamps_the_latest_revision() {
    let mut c = folded(&[("revision", revision("A", "base", "base", true))]);
    assert!(!c.is_partial());
    let e = entry(1, "partial", &serde_json::json!({"partial": true}));
    fold(&mut c, e);
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
    // An agent note carries no review_id — that is what marks it agent-authored.
    assert_eq!(c.threads[0].comments[0].review_id, None);
}

fn cinput(thread_id: Option<u64>, body: &str) -> CommentInput {
    CommentInput {
        thread_id,
        revision: None,
        file: None,
        line: None,
        side: None,
        range: None,
        line_text: None,
        body: body.to_string(),
        resolved: None,
    }
}

#[test]
fn mint_thread_id_assigns_then_keeps_the_counter_ahead() {
    let mut c = ChangeProj::empty(&change_row());
    // A new-thread comment is minted the next id, advancing the counter once.
    let mut open = cinput(None, "opens");
    c.mint_thread_id(&mut open);
    assert_eq!(open.thread_id, Some(0));
    assert_eq!(c.next_thread_id, 1);
    // A reply already names its thread: not re-minted, counter unmoved.
    let mut reply = cinput(Some(0), "reply");
    c.mint_thread_id(&mut reply);
    assert_eq!(reply.thread_id, Some(0));
    assert_eq!(c.next_thread_id, 1);
    // An empty new-thread comment is left unset.
    let mut empty = cinput(None, "");
    c.mint_thread_id(&mut empty);
    assert_eq!(empty.thread_id, None);
    assert_eq!(c.next_thread_id, 1);
    // A stamped id past the counter (a replayed open) pulls it forward.
    let mut stamped = cinput(Some(5), "stamped");
    c.mint_thread_id(&mut stamped);
    assert_eq!(c.next_thread_id, 6);
}

#[test]
fn fold_opens_a_thread_for_a_stamped_unseen_id() {
    let mut c = ChangeProj::empty(&change_row());
    fold(
        &mut c,
        entry(0, "revision", &revision("A", "base", "base", true)),
    );
    // A comment carrying a stamped id for a thread not seen yet opens it, and
    // the mint counter advances past the stamped id.
    let open = entry(
        1,
        "comment",
        &serde_json::json!({"thread_id": 3, "revision": 0, "file": "a.rs", "line": 1, "side": "new", "body": "why?"}),
    );
    fold(&mut c, open);
    assert_eq!(c.threads.len(), 1);
    assert_eq!(c.threads[0].id, 3);
    assert_eq!(c.next_thread_id, 4);
    // A later comment on the same id replies rather than re-opening.
    let reply = entry(
        2,
        "comment",
        &serde_json::json!({"thread_id": 3, "body": "ok"}),
    );
    fold(&mut c, reply);
    assert_eq!(c.threads.len(), 1);
    assert_eq!(c.threads[0].comments.len(), 2);
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
