use nit_types::domain::{ChangeStatus, Verdict};
use nit_types::log::{LogPayload, ReviewPayload, RevisionPayload};

use super::*;

fn change_row() -> db::ChangeRow {
    db::ChangeRow {
        id: 1,
        repo_id: 1,
        change_key: "Iabc".into(),
        status: None,
        created_at: "t0".to_string(),
    }
}

fn revision(sha: &str) -> LogPayload {
    LogPayload::Revision(RevisionPayload {
        commit_sha: sha.to_string(),
        parent_sha: "base".to_string(),
        fork_sha: "base".to_string(),
        message: format!("subject {sha}\n\nChange-Id: Iabc\n"),
        resets_status: true,
    })
}

fn review(revision: u64, verdict: Verdict) -> LogPayload {
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
    let rows: Vec<db::LogRow> = [revision("A"), review(0, Verdict::Approve)]
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
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(c.reviews[0].id, 1, "the review entry sits at position 1");
}
