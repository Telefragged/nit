use super::*;

fn chain_row() -> db::ChainRow {
    db::ChainRow {
        id: 1,
        repo_id: 1,
        git_dir: "/r/.git".to_string(),
        branch: "feat".to_string(),
        base: "main".to_string(),
        created_at: "t0".to_string(),
    }
}

fn entry(idx: u64, kind: &str, payload: serde_json::Value) -> Entry {
    Entry {
        idx,
        kind: kind.to_string(),
        payload,
        created_at: format!("t{idx}"),
    }
}

fn revisions(live: serde_json::Value, added: serde_json::Value) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("live".to_string(), live);
    m.insert("added".to_string(), added);
    serde_json::Value::Object(m)
}

/// A push creating two changes, then an amend of the first.
#[test]
fn push_creates_changes_then_amend_adds_revision() {
    let mut p = Projection::empty(&chain_row());
    fold(
        &mut p,
        &entry(
            0,
            "revisions",
            revisions(
                serde_json::json!([
                    {"change_key": "Iaaa", "change_id": 10, "position": 0},
                    {"change_key": "Ibbb", "change_id": 11, "position": 1}
                ]),
                serde_json::json!([
                    {"change_key": "Iaaa", "number": 1, "commit_sha": "a1", "parent_sha": "p", "message": "a", "resets_status": true},
                    {"change_key": "Ibbb", "number": 1, "commit_sha": "b1", "parent_sha": "a1", "message": "b", "resets_status": true}
                ]),
            ),
        ),
    )
    .expect("fold push");
    assert_eq!(p.changes.len(), 2);
    assert_eq!(p.head, 1);
    assert_eq!(p.change_by_key("Iaaa").expect("a").id, 10);
    assert_eq!(p.change_by_key("Iaaa").expect("a").revisions.len(), 1);
    assert_eq!(derive_state(&p), "waiting_for_review");

    // Approve Iaaa, then amend it (non-pure) → status resets to pending.
    fold(
        &mut p,
        &entry(
            1,
            "review",
            serde_json::json!({"change_key": "Iaaa", "review_id": 20, "revision": 1,
                "verdict": "approve", "message": "ok", "comments": []}),
        ),
    )
    .expect("fold review");
    assert_eq!(p.change_by_key("Iaaa").expect("a").status, Status::Approved);

    fold(
        &mut p,
        &entry(
            2,
            "revisions",
            revisions(
                serde_json::json!([
                    {"change_key": "Iaaa", "change_id": 10, "position": 0},
                    {"change_key": "Ibbb", "change_id": 11, "position": 1}
                ]),
                serde_json::json!([
                    {"change_key": "Iaaa", "number": 2, "commit_sha": "a2", "parent_sha": "p", "message": "a v2", "resets_status": true}
                ]),
            ),
        ),
    )
    .expect("fold amend");
    let a = p.change_by_key("Iaaa").expect("a");
    assert_eq!(a.revisions.len(), 2);
    assert_eq!(a.status, Status::Pending, "non-pure amend resets status");
}

#[test]
fn pure_rebase_keeps_status() {
    let mut p = Projection::empty(&chain_row());
    fold(&mut p, &push_one("Iaaa", 10, 1, "a1")).expect("push");
    fold(
        &mut p,
        &entry(
            1,
            "review",
            serde_json::json!({"change_key": "Iaaa", "review_id": 20, "revision": 1,
                "verdict": "approve", "message": "", "comments": []}),
        ),
    )
    .expect("review");
    // resets_status:false models a pure rebase → status retained.
    fold(
        &mut p,
        &entry(
            2,
            "revisions",
            revisions(
                serde_json::json!([{"change_key": "Iaaa", "change_id": 10, "position": 0}]),
                serde_json::json!([{"change_key": "Iaaa", "number": 2, "commit_sha": "a2",
                    "parent_sha": "p2", "message": "a1", "resets_status": false}]),
            ),
        ),
    )
    .expect("rebase");
    assert_eq!(p.change_by_key("Iaaa").expect("a").status, Status::Approved);
}

fn push_one(key: &str, id: u64, number: u64, sha: &str) -> Entry {
    entry(
        0,
        "revisions",
        revisions(
            serde_json::json!([{"change_key": key, "change_id": id, "position": 0}]),
            serde_json::json!([{"change_key": key, "number": number, "commit_sha": sha,
                "parent_sha": "p", "message": "m", "resets_status": true}]),
        ),
    )
}

#[test]
fn orphan_retains_status_then_reattaches() {
    let mut p = Projection::empty(&chain_row());
    fold(&mut p, &push_one("Iaaa", 10, 1, "a1")).expect("push");
    fold(
        &mut p,
        &entry(
            1,
            "review",
            serde_json::json!({"change_key": "Iaaa", "review_id": 20, "revision": 1,
                "verdict": "request_changes", "message": "", "comments": []}),
        ),
    )
    .expect("review");
    // A push with an empty live set orphans Iaaa.
    fold(
        &mut p,
        &entry(
            2,
            "revisions",
            revisions(serde_json::json!([]), serde_json::json!([])),
        ),
    )
    .expect("orphan");
    let a = p.change_by_key("Iaaa").expect("a");
    assert!(a.orphaned);
    assert_eq!(a.status_str(), "orphaned");
    assert_eq!(a.status, Status::ChangesRequested, "status retained");
    // Reattach (same sha → no new revision) exposes the retained status.
    fold(
        &mut p,
        &entry(
            3,
            "revisions",
            revisions(
                serde_json::json!([{"change_key": "Iaaa", "change_id": 10, "position": 0}]),
                serde_json::json!([]),
            ),
        ),
    )
    .expect("reattach");
    let a = p.change_by_key("Iaaa").expect("a");
    assert!(!a.orphaned);
    assert_eq!(a.status_str(), "changes_requested");
}

#[test]
fn review_opens_threads_and_carries_drafted_resolution() {
    let mut p = Projection::empty(&chain_row());
    fold(&mut p, &push_one("Iaaa", 10, 1, "a1")).expect("push");
    // A review opening a thread staged resolved publishes it already resolved
    // (the reviewer checked the box at draft time), pinned to revision 1.
    fold(
        &mut p,
        &entry(
            1,
            "review",
            serde_json::json!({"change_key": "Iaaa", "review_id": 20, "revision": 1,
                "verdict": "comment", "message": "",
                "comments": [{"thread_id": null, "revision": 1, "file": "m.rs", "line": 2,
                    "side": "new", "range": null, "line_text": "x", "body": "nit",
                    "resolved": true}]}),
        ),
    )
    .expect("review");
    let a = p.change_by_key("Iaaa").expect("a");
    assert_eq!(a.threads.len(), 1);
    let t = &a.threads[0];
    assert_eq!(t.id, 0, "first thread minted id 0");
    assert_eq!(t.comments[0].author, "reviewer");
    assert_eq!(t.revision, 1, "pinned to the authored revision");
    assert_eq!(t.comments.len(), 1);
    assert_eq!(t.comments[0].review_id, Some(20));
    assert!(t.resolved, "staged resolution applies on publish");
    assert_eq!(a.unresolved_threads(), 0);

    // A later review reopens it with an empty-body resolution-only reply: it
    // moves the thread's state without materializing a comment.
    fold(
        &mut p,
        &entry(
            2,
            "review",
            serde_json::json!({"change_key": "Iaaa", "review_id": 21, "revision": 1,
                "verdict": "comment", "message": "",
                "comments": [{"thread_id": 0, "body": "", "resolved": false}]}),
        ),
    )
    .expect("reopen review");
    let a = p.change_by_key("Iaaa").expect("a");
    assert_eq!(
        a.threads[0].comments.len(),
        1,
        "empty-body resolution adds no comment"
    );
    assert!(!a.threads[0].resolved, "thread reopened");
    assert_eq!(a.unresolved_threads(), 1);
}

#[test]
fn agent_comment_replies_toggle_thread_resolution() {
    let mut p = Projection::empty(&chain_row());
    fold(&mut p, &push_one("Iaaa", 10, 1, "a1")).expect("push");
    // A reviewer opens a thread requesting changes.
    fold(
        &mut p,
        &entry(
            1,
            "review",
            serde_json::json!({"change_key": "Iaaa", "review_id": 20, "revision": 1,
                "verdict": "request_changes", "message": "fix",
                "comments": [{"thread_id": null, "revision": 1, "file": "m.rs", "line": 2,
                    "side": "new", "range": null, "line_text": "x", "body": "why"}]}),
        ),
    )
    .expect("review");

    // An agent `comment` replying with resolved:true appends a comment and
    // resolves the thread.
    fold(
        &mut p,
        &entry(
            2,
            "comment",
            serde_json::json!({"change_key": "Iaaa", "thread_id": 0, "body": "done", "resolved": true}),
        ),
    )
    .expect("agent resolve");
    let t = &p.change_by_key("Iaaa").expect("a").threads[0];
    assert!(t.resolved);
    assert_eq!(t.comments.len(), 2, "agent reply appended");
    assert_eq!(t.comments[1].author, "agent");
    assert_eq!(t.comments[1].body, "done");
    assert_eq!(t.comments[1].review_id, None);

    // A later comment with resolved:false reopens it.
    fold(
        &mut p,
        &entry(
            3,
            "comment",
            serde_json::json!({"change_key": "Iaaa", "thread_id": 0, "body": "actually…", "resolved": false}),
        ),
    )
    .expect("reopen");
    assert!(!p.change_by_key("Iaaa").expect("a").threads[0].resolved);

    // A plain comment (no decision) leaves the state unchanged.
    fold(
        &mut p,
        &entry(
            4,
            "comment",
            serde_json::json!({"change_key": "Iaaa", "thread_id": 0, "body": "ok"}),
        ),
    )
    .expect("plain reply");
    assert!(!p.change_by_key("Iaaa").expect("a").threads[0].resolved);
}

#[test]
fn agent_thread_leaves_status_untouched_and_mints_ids() {
    let mut p = Projection::empty(&chain_row());
    fold(&mut p, &push_one("Iaaa", 10, 1, "a1")).expect("push");
    // An agent opens a thread on its own change, born resolved (a
    // for-the-record note) — it is not a verdict, so the status stays pending.
    fold(
        &mut p,
        &entry(
            1,
            "comment",
            serde_json::json!({"change_key": "Iaaa", "thread_id": null, "revision": 1,
                "file": "Cargo.toml", "line": 2, "side": "new", "range": null,
                "line_text": "serde = …", "body": "why this dep", "resolved": true}),
        ),
    )
    .expect("agent comment");
    let a = p.change_by_key("Iaaa").expect("a");
    assert_eq!(a.status, Status::Pending, "an agent note is not a verdict");
    assert_eq!(a.threads.len(), 1);
    let t = &a.threads[0];
    assert_eq!(t.id, 0);
    assert_eq!(t.comments[0].author, "agent");
    assert_eq!(t.revision, 1);
    assert!(t.resolved, "born resolved");
    assert_eq!(t.comments[0].review_id, None);
    assert_eq!(a.unresolved_threads(), 0);

    // A second opened thread mints id 1 (creation order) and is open.
    fold(
        &mut p,
        &entry(
            2,
            "comment",
            serde_json::json!({"change_key": "Iaaa", "thread_id": null, "file": "m.rs",
                "line": 1, "side": "new", "body": "another"}),
        ),
    )
    .expect("agent comment 2");
    let a = p.change_by_key("Iaaa").expect("a");
    assert_eq!(a.threads.len(), 2);
    assert_eq!(a.threads[1].id, 1);
    assert!(!a.threads[1].resolved, "open by default");
    assert_eq!(a.unresolved_threads(), 1);
}

#[test]
fn thread_ids_are_deterministic_across_replay() {
    let rows = vec![
        db::LogRow {
            idx: 0,
            kind: "revisions".to_string(),
            payload: revisions(
                serde_json::json!([{"change_key": "Iaaa", "change_id": 10, "position": 0}]),
                serde_json::json!([{"change_key": "Iaaa", "number": 1, "commit_sha": "a1",
                    "parent_sha": "p", "message": "m", "resets_status": true}]),
            )
            .to_string(),
            created_at: "t0".to_string(),
        },
        db::LogRow {
            idx: 1,
            kind: "comment".to_string(),
            payload: serde_json::json!({"change_key": "Iaaa", "thread_id": null,
                "file": "m.rs", "line": 1, "side": "new", "body": "first"})
            .to_string(),
            created_at: "t1".to_string(),
        },
        db::LogRow {
            idx: 2,
            kind: "comment".to_string(),
            payload:
                serde_json::json!({"change_key": "Iaaa", "thread_id": 0, "body": "reply to 0"})
                    .to_string(),
            created_at: "t2".to_string(),
        },
    ];
    let p = replay(&chain_row(), &rows).expect("replay");
    let a = p.change_by_key("Iaaa").expect("a");
    assert_eq!(a.threads.len(), 1, "the second comment joined thread 0");
    assert_eq!(a.threads[0].comments.len(), 2);
    assert_eq!(p.next_thread_id, 1, "one thread minted");
    // max_assigned_id ignores thread ids — they are never stored.
    assert_eq!(
        max_assigned_id(&rows).expect("max"),
        10,
        "only change/review ids resume the counter"
    );
}

#[test]
fn partial_and_closed_drive_state() {
    let mut p = Projection::empty(&chain_row());
    fold(&mut p, &push_one("Iaaa", 10, 1, "a1")).expect("push");
    fold(
        &mut p,
        &entry(1, "partial", serde_json::json!({"partial": true})),
    )
    .expect("partial");
    fold(
        &mut p,
        &entry(
            2,
            "review",
            serde_json::json!({"change_key": "Iaaa", "review_id": 20, "revision": 1,
                "verdict": "approve", "message": "", "comments": []}),
        ),
    )
    .expect("approve");
    assert_eq!(derive_state(&p), "agents_turn", "all approved but partial");
    fold(
        &mut p,
        &entry(3, "partial", serde_json::json!({"partial": false})),
    )
    .expect("ready");
    assert_eq!(derive_state(&p), "approved");
    fold(
        &mut p,
        &entry(4, "chain_closed", serde_json::json!({"status": "merged"})),
    )
    .expect("closed");
    assert_eq!(derive_state(&p), "merged");
}

#[test]
fn replay_equals_incremental_fold() {
    let rows = vec![
        db::LogRow {
            idx: 0,
            kind: "revisions".to_string(),
            payload: revisions(
                serde_json::json!([{"change_key": "Iaaa", "change_id": 10, "position": 0}]),
                serde_json::json!([{"change_key": "Iaaa", "number": 1, "commit_sha": "a1",
                    "parent_sha": "p", "message": "m", "resets_status": true}]),
            )
            .to_string(),
            created_at: "t0".to_string(),
        },
        db::LogRow {
            idx: 1,
            kind: "review".to_string(),
            payload: serde_json::json!({"change_key": "Iaaa", "review_id": 20, "revision": 1,
                "verdict": "approve", "message": "", "comments": []})
            .to_string(),
            created_at: "t1".to_string(),
        },
    ];
    let p = replay(&chain_row(), &rows).expect("replay");
    assert_eq!(p.head, 2);
    assert_eq!(p.change_by_key("Iaaa").expect("a").status, Status::Approved);
    assert_eq!(max_assigned_id(&rows).expect("max"), 20);
}
