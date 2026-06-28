use nit_types::enums::{ChangeStatus, Verdict};
use nit_types::log::{LogPayload, ReviewPayload, RevisionPayload};

use super::*;

fn change_row() -> db::ChangeRow {
    db::ChangeRow {
        id: 1,
        repo_id: 1,
        change_key: "Iabc".to_string(),
        status: None,
        created_at: "t0".to_string(),
    }
}

fn revision(sha: &str) -> LogPayload {
    LogPayload::Revision(RevisionPayload {
        commit_sha: sha.to_string(),
        parent_sha: "base".to_string(),
        base_sha: "base".to_string(),
        message: format!("subject {sha}\n\nChange-Id: Iabc\n"),
        resets_status: true,
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

/// The storage boundary round-trips: payloads serialized into `db::LogRow`s
/// replay to the same projection, the review id among them.
#[test]
fn replay_rows_round_trips_stored_log() {
    let rows: Vec<db::LogRow> = [revision("A"), review(0, Verdict::Approve)]
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
    let c = replay_rows(&change_row(), &rows).expect("replay");
    assert_eq!(c.revisions.len(), 1);
    assert_eq!(c.status_at(0), ChangeStatus::Approved);
    assert_eq!(c.reviews.iter().map(|r| r.id).max(), Some(100));
}
