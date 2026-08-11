//! Serde round-trip tests, run by the `test-nit-types` flake check with no
//! optional features — exercising the serde-only baseline that an optional
//! feature (clap, ts) would otherwise mask.

use crate::domain::ChangeNumber;
use crate::domain::{LifecycleAction, Side};
use crate::domain::{LifecyclePayload, LogEntry, LogPayload, RevisionPayload};

fn revision_entry() -> LogEntry {
    LogEntry {
        change_number: ChangeNumber(7),
        position: 2,
        sequence: 42,
        created_at: "t".to_string(),
        payload: LogPayload::Revision(RevisionPayload {
            commit_sha: "a".into(),
            parent_sha: "b".into(),
            fork_sha: "c".into(),
            message: "m".to_string(),
            resets_status: true,
        }),
    }
}

#[test]
fn log_entry_flattens_to_an_adjacent_tag() {
    let json = serde_json::to_string(&revision_entry()).expect("serialize");
    assert_eq!(
        json,
        r#"{"change_number":7,"position":2,"sequence":42,"created_at":"t","kind":"revision","payload":{"commit_sha":"a","parent_sha":"b","fork_sha":"c","message":"m","resets_status":true}}"#
    );
}

#[test]
fn log_entry_round_trips() {
    let json = serde_json::to_string(&revision_entry()).expect("serialize");
    let back: LogEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.sequence, 42);
    match back.payload {
        LogPayload::Revision(p) => assert_eq!(p.commit_sha, "a"),
        _ => panic!("expected a revision payload"),
    }
}

#[test]
fn payload_serializes_as_the_bare_inner_struct() {
    // The storage boundary serializes the inner struct alone (kind goes in its
    // own column) — never the adjacently-tagged LogPayload wrapper.
    let p = RevisionPayload {
        commit_sha: "a".into(),
        parent_sha: "b".into(),
        fork_sha: "c".into(),
        message: "m".to_string(),
        resets_status: true,
    };
    let json = serde_json::to_string(&p).expect("serialize");
    assert_eq!(
        json,
        r#"{"commit_sha":"a","parent_sha":"b","fork_sha":"c","message":"m","resets_status":true}"#
    );
}

#[test]
fn lifecycle_skips_absent_fields_under_flatten() {
    let entry = LogEntry {
        change_number: ChangeNumber(1),
        position: 0,
        sequence: 0,
        created_at: "t".to_string(),
        payload: LogPayload::Lifecycle(LifecyclePayload {
            action: LifecycleAction::Merged,
            commit_sha: None,
            message: None,
        }),
    };
    let json = serde_json::to_string(&entry).expect("serialize");
    assert_eq!(
        json,
        r#"{"change_number":1,"position":0,"sequence":0,"created_at":"t","kind":"lifecycle","payload":{"action":"merged"}}"#
    );
}

#[test]
fn side_round_trips_without_clap() {
    assert_eq!(
        serde_json::to_string(&Side::New).expect("serialize"),
        r#""new""#
    );
    assert_eq!(
        serde_json::from_str::<Side>(r#""old""#).expect("deserialize"),
        Side::Old
    );
}

#[test]
fn client_msg_subscribe_is_externally_tagged() {
    use crate::events::ClientMessage;
    use std::collections::HashMap;
    let map = HashMap::from([("10".to_string(), 5u64)]);
    let json = serde_json::to_string(&ClientMessage::Subscribe(map)).expect("serialize");
    assert_eq!(json, r#"{"subscribe":{"10":5}}"#);
}
