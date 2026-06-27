use super::*;
use crate::enums::{ChangeStatus, LifecycleAction, Side, Verdict};

fn change_row() -> db::ChangeRow {
    db::ChangeRow {
        id: 1,
        repo_id: 1,
        change_key: "Iabc".to_string(),
        status: None,
        created_at: "t0".to_string(),
    }
}

fn entry(idx: u64, payload: LogPayload) -> Entry {
    Entry {
        seq: idx,
        idx,
        payload,
        created_at: format!("t{idx}"),
    }
}

/// A `revision` payload; the fold mints its 0-based number.
fn revision(sha: &str, parent: &str, base: &str, resets: bool) -> LogPayload {
    LogPayload::Revision(RevisionPayload {
        commit_sha: sha.to_string(),
        parent_sha: parent.to_string(),
        base_sha: base.to_string(),
        message: format!("subject {sha}\n\nChange-Id: Iabc\n"),
        resets_status: resets,
    })
}

fn review(revision: u64, verdict: Verdict) -> LogPayload {
    LogPayload::Review(ReviewPayload {
        review_id: 100 + revision,
        revision,
        verdict,
        message: "msg".to_string(),
        comments: vec![],
    })
}

/// A new-thread comment anchored at `file:line` on the new side of revision 0.
fn anchored(file: &str, line: u64, body: &str) -> CommentInput {
    CommentInput {
        thread_id: None,
        revision: Some(0),
        file: Some(file.to_string()),
        line: Some(line),
        side: Some(Side::New),
        range: None,
        line_text: None,
        body: body.to_string(),
        resolved: None,
    }
}

/// Fold a sequence of payloads onto a fresh change, idx-ordered.
fn folded(payloads: Vec<LogPayload>) -> ChangeProj {
    let mut c = ChangeProj::empty(&change_row());
    for (i, payload) in payloads.into_iter().enumerate() {
        let e = entry(u64::try_from(i).expect("index fits u64"), payload);
        fold(&mut c, e);
    }
    c
}

#[test]
fn revisions_are_zero_based_and_minted_in_the_fold() {
    let c = folded(vec![
        revision("A", "base", "base", true),
        revision("B", "A", "base", true),
    ]);
    assert_eq!(c.revisions.len(), 2);
    assert_eq!(c.revisions[0].number, 0);
    assert_eq!(c.revisions[1].number, 1);
    assert_eq!(c.latest_revision().expect("a revision").commit_sha, "B");
}

#[test]
fn status_is_per_revision() {
    let c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::RequestChanges),
        revision("B", "base", "base", true), // reword: new patchset
    ]);
    // The review landed on r0; r1 has no review yet and resets → pending.
    assert_eq!(c.status_at(0), ChangeStatus::ChangesRequested);
    assert_eq!(c.status_at(1), ChangeStatus::Pending);
}

#[test]
fn pure_rebase_carries_status_forward() {
    let c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::Approve),
        // r1 is a pure rebase (resets_status = false): inherits r0's approve.
        revision("B", "base2", "base2", false),
    ]);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(c.status_at(1), ChangeStatus::Approved);
}

#[test]
fn reword_resets_status() {
    let c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::Approve),
        // r1 is a reword (resets_status = true): back to pending.
        revision("B", "base", "base", true),
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
    let c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::Approve),
        revision("B", "base", "base", true),
    ]);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(c.current_status(), ChangeStatus::Pending);
    // A verdict on the latest revision moves the current status.
    let c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::Approve),
    ]);
    assert_eq!(c.current_status(), ChangeStatus::Approved);
    // The lifecycle overlay wins change-wide: abandoned regardless of revision.
    let c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::Approve),
        LogPayload::lifecycle(LifecycleAction::Abandoned, None, None),
    ]);
    assert_eq!(c.current_status(), ChangeStatus::Abandoned);
}

#[test]
fn merged_is_per_revision() {
    let c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::Approve),
        revision("B", "base", "base", true),
        LogPayload::lifecycle(LifecycleAction::Merged, Some(1), None),
    ]);
    // Only the landed revision shows merged; older ones show their own status.
    assert_eq!(c.status_at(1), ChangeStatus::Merged);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert!(c.is_terminal());
}

#[test]
fn abandon_then_reopen() {
    let mut c = folded(vec![
        revision("A", "base", "base", true),
        review(0, Verdict::RequestChanges),
        LogPayload::lifecycle(LifecycleAction::Abandoned, None, None),
    ]);
    assert_eq!(c.status_at(0), ChangeStatus::Abandoned);
    assert!(c.is_terminal());
    // Reopen restores the retained verdict status.
    let e = entry(
        3,
        LogPayload::lifecycle(LifecycleAction::Reopened, None, None),
    );
    fold(&mut c, e);
    assert!(!c.is_terminal());
    assert_eq!(c.status_at(0), ChangeStatus::ChangesRequested);
}

#[test]
fn threads_open_reply_and_resolve() {
    let c = folded(vec![
        revision("A", "base", "base", true),
        // A review opening a thread (reviewer), then a resolving reply.
        LogPayload::Review(ReviewPayload {
            review_id: 200,
            revision: 0,
            verdict: Verdict::Comment,
            message: String::new(),
            comments: vec![anchored("src/x.rs", 3, "look")],
        }),
        LogPayload::Review(ReviewPayload {
            review_id: 201,
            revision: 0,
            verdict: Verdict::Approve,
            message: String::new(),
            comments: vec![CommentInput {
                thread_id: Some(0),
                resolved: Some(true),
                ..cinput(None, "fixed")
            }],
        }),
    ]);
    assert_eq!(c.threads.len(), 1);
    assert_eq!(c.threads[0].comments.len(), 2);
    assert!(c.threads[0].resolved);
}

#[test]
fn agent_comment_opens_a_thread() {
    let c = folded(vec![
        revision("A", "base", "base", true),
        LogPayload::Comment(anchored("a.rs", 1, "why?")),
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
    fold(&mut c, entry(0, revision("A", "base", "base", true)));
    // A comment carrying a stamped id for a thread not seen yet opens it, and
    // the mint counter advances past the stamped id.
    let open = entry(
        1,
        LogPayload::Comment(CommentInput {
            thread_id: Some(3),
            ..anchored("a.rs", 1, "why?")
        }),
    );
    fold(&mut c, open);
    assert_eq!(c.threads.len(), 1);
    assert_eq!(c.threads[0].id, 3);
    assert_eq!(c.next_thread_id, 4);
    // A later comment on the same id replies rather than re-opening.
    let reply = entry(2, LogPayload::Comment(cinput(Some(3), "ok")));
    fold(&mut c, reply);
    assert_eq!(c.threads.len(), 1);
    assert_eq!(c.threads[0].comments.len(), 2);
}

#[test]
fn replay_round_trips() {
    let rows: Vec<db::LogRow> = [
        revision("A", "base", "base", true),
        review(0, Verdict::Approve),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, payload)| db::LogRow {
        seq: u64::try_from(i).expect("index fits u64"),
        idx: u64::try_from(i).expect("index fits u64"),
        kind: payload.kind().as_str().to_string(),
        payload: payload_to_json(&payload).expect("serialize payload"),
        created_at: format!("t{i}"),
    })
    .collect();
    let c = replay(&change_row(), &rows).expect("replay");
    assert_eq!(c.revisions.len(), 1);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(max_assigned_id(&rows).expect("max"), 100);
}
