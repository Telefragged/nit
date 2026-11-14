use nit_types::domain::{ChangeId, RevisionNumber, Sha};
use nit_types::domain::{ChangeStatus, Verdict};
use nit_types::domain::{LogPayload, ReviewPayload, RevisionPayload};

use super::*;

fn change_row() -> db::ChangeRow {
    db::ChangeRow {
        id: ChangeNumber::new(1),
        repo_id: 1,
        change_id: ChangeId::new("Iabc0000000000000000000000000000000000000")
            .expect("valid change id"),
        status: None,
        created_at: "t0".to_string(),
    }
}

fn revision(commit: &str) -> LogPayload {
    let base = Sha::new("ba5eba5eba5eba5eba5eba5eba5eba5eba5eba5e").expect("valid sha");
    LogPayload::Revision(RevisionPayload {
        commit_sha: Sha::new(commit).expect("valid sha"),
        parent_sha: base.clone(),
        fork_sha: base,
        message: "subject\n\nChange-Id: Iabc0000000000000000000000000000000000000\n".to_string(),
        resets_status: true,
    })
}

fn review(revision: RevisionNumber, verdict: Verdict) -> LogPayload {
    LogPayload::Review(ReviewPayload {
        revision,
        verdict,
        message: "msg".to_string(),
        comments: vec![],
    })
}

/// The storage boundary round-trips: payloads serialized into `db::LogRow`s
/// replay to the same projection — the review's id among them, which is the
/// `position` of the row it came from and so needs nothing stored to survive.
#[test]
fn replay_rows_round_trips_stored_log() {
    let rows: Vec<db::LogRow> = [
        revision("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"),
        review(RevisionNumber::new(0), Verdict::Approve),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, payload)| db::LogRow {
        sequence: u64::try_from(i).expect("index fits u64"),
        position: u64::try_from(i).expect("index fits u64"),
        kind: payload.kind().as_str().to_string(),
        payload: payload_to_json(&payload).expect("serialize payload"),
        created_at: format!("t{i}"),
    })
    .collect();
    let c = replay_rows(&change_row(), &rows).expect("replay");
    assert_eq!(c.revisions.len(), 1);
    assert_eq!(c.status_at(RevisionNumber::new(0)), ChangeStatus::Approved);
    assert_eq!(c.reviews[0].id, 1, "the review entry sits at position 1");
}
