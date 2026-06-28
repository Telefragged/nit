//! Serde round-trip tests, run by the `test-nit-types` flake check with no
//! optional features — exercising the serde-only baseline that an optional
//! feature (clap, ts) would otherwise mask.

use crate::enums::{LifecycleAction, Side};
use crate::log::{LifecyclePayload, LogEntry, LogPayload, RevisionPayload};

fn revision_entry() -> LogEntry {
    LogEntry {
        change_id: 7,
        idx: 2,
        seq: 42,
        created_at: "t".to_string(),
        payload: LogPayload::Revision(RevisionPayload {
            commit_sha: "a".to_string(),
            parent_sha: "b".to_string(),
            base_sha: "c".to_string(),
            message: "m".to_string(),
            resets_status: true,
        }),
    }
}

#[test]
fn log_entry_flattens_to_an_adjacent_tag() {
    // The flattened LogPayload contributes the sibling `kind` + `payload` keys,
    // after the entry's own fields — the exact wire shape.
    let json = serde_json::to_string(&revision_entry()).expect("serialize");
    assert_eq!(
        json,
        r#"{"change_id":7,"idx":2,"seq":42,"created_at":"t","kind":"revision","payload":{"commit_sha":"a","parent_sha":"b","base_sha":"c","message":"m","resets_status":true}}"#
    );
}

#[test]
fn log_entry_round_trips() {
    let json = serde_json::to_string(&revision_entry()).expect("serialize");
    let back: LogEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.seq, 42);
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
        commit_sha: "a".to_string(),
        parent_sha: "b".to_string(),
        base_sha: "c".to_string(),
        message: "m".to_string(),
        resets_status: true,
    };
    let json = serde_json::to_string(&p).expect("serialize");
    assert_eq!(
        json,
        r#"{"commit_sha":"a","parent_sha":"b","base_sha":"c","message":"m","resets_status":true}"#
    );
}

#[test]
fn lifecycle_skips_absent_fields_under_flatten() {
    let entry = LogEntry {
        change_id: 1,
        idx: 0,
        seq: 0,
        created_at: "t".to_string(),
        payload: LogPayload::Lifecycle(LifecyclePayload {
            action: LifecycleAction::Merged,
            revision: None,
            message: None,
        }),
    };
    let json = serde_json::to_string(&entry).expect("serialize");
    assert_eq!(
        json,
        r#"{"change_id":1,"idx":0,"seq":0,"created_at":"t","kind":"lifecycle","payload":{"action":"merged"}}"#
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
fn stream_msg_untagged_falls_through() {
    use crate::events::{NewParent, StreamMsg};
    // An entry frame (carrying the entry's own fields) parses as Entry.
    let entry_json = serde_json::to_string(&revision_entry()).expect("serialize");
    match serde_json::from_str::<StreamMsg>(&entry_json).expect("entry frame") {
        StreamMsg::Entry(e) => assert_eq!(e.change_id, 7),
        StreamMsg::NewParent { .. } => panic!("expected an entry"),
    }
    // A new_parent frame lacks the entry fields, so it falls through to it.
    let np = StreamMsg::NewParent {
        new_parent: NewParent { of: 10, parent: 9 },
    };
    let np_json = serde_json::to_string(&np).expect("serialize");
    assert_eq!(np_json, r#"{"new_parent":{"of":10,"parent":9}}"#);
    match serde_json::from_str::<StreamMsg>(&np_json).expect("new_parent frame") {
        StreamMsg::NewParent { new_parent } => {
            assert_eq!(new_parent.of, 10);
            assert_eq!(new_parent.parent, 9);
        }
        StreamMsg::Entry(_) => panic!("expected a new_parent"),
    }
}

#[test]
fn client_msg_subscribe_is_externally_tagged() {
    use crate::events::ClientMsg;
    use std::collections::HashMap;
    let map = HashMap::from([("10".to_string(), 5u64)]);
    let json = serde_json::to_string(&ClientMsg::Subscribe(map)).expect("serialize");
    assert_eq!(json, r#"{"subscribe":{"10":5}}"#);
}
