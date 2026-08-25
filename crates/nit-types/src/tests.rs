//! Serde round-trip tests, run by the `test-nit-types` flake check with no
//! optional features — exercising the serde-only baseline that an optional
//! feature (clap, ts) would otherwise mask.

use crate::domain::ChangeNumber;
use crate::domain::{Anchor, CommentInput, CommentRange, LineAnchor};
use crate::domain::{ChangeId, Sha};
use crate::domain::{LifecycleAction, Side};
use crate::domain::{LifecyclePayload, LogEntry, LogPayload, RevisionPayload};

/// A distinct, well-formed object name for `label`.
///
/// Fixtures name commits `A1`, `base`, or a hex prefix they assert on;
/// git names them in 40 hex characters, and [`Sha`] holds them to it.
pub(crate) fn sha(label: &str) -> Sha {
    Sha::new(hex_expansion(label)).expect("a hex expansion of the label")
}

/// A distinct, well-formed change identity for `label`.
pub(crate) fn change_id(label: &str) -> ChangeId {
    let body = hex_expansion(label.strip_prefix('I').unwrap_or(label));
    ChangeId::new(format!("I{body}")).expect("a hex expansion of the label")
}

/// `label` as 40 hex characters: itself when it is already hex, its bytes
/// otherwise, zero-padded either way.
fn hex_expansion(label: &str) -> String {
    let hex: String = if label.chars().all(|c| c.is_ascii_hexdigit()) {
        label.to_string()
    } else {
        label.bytes().fold(String::new(), |mut hex, b| {
            use std::fmt::Write;
            let _ = write!(hex, "{b:02x}");
            hex
        })
    };
    format!("{hex:0<40}")[..40].to_string()
}

fn revision_entry() -> LogEntry {
    LogEntry {
        change_number: ChangeNumber::new(7),
        position: 2,
        sequence: 42,
        created_at: "t".to_string(),
        payload: LogPayload::Revision(RevisionPayload {
            commit_sha: sha("a"),
            parent_sha: sha("b"),
            fork_sha: sha("c"),
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
        format!(
            r#"{{"change_number":7,"position":2,"sequence":42,"created_at":"t","kind":"revision","payload":{{"commit_sha":"{}","parent_sha":"{}","fork_sha":"{}","message":"m","resets_status":true}}}}"#,
            sha("a"),
            sha("b"),
            sha("c")
        )
    );
}

#[test]
fn log_entry_round_trips() {
    let json = serde_json::to_string(&revision_entry()).expect("serialize");
    let back: LogEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.sequence, 42);
    match back.payload {
        LogPayload::Revision(p) => assert_eq!(p.commit_sha, sha("a")),
        _ => panic!("expected a revision payload"),
    }
}

#[test]
fn payload_serializes_as_the_bare_inner_struct() {
    // The storage boundary serializes the inner struct alone (kind goes in its
    // own column) — never the adjacently-tagged LogPayload wrapper.
    let p = RevisionPayload {
        commit_sha: sha("a"),
        parent_sha: sha("b"),
        fork_sha: sha("c"),
        message: "m".to_string(),
        resets_status: true,
    };
    let json = serde_json::to_string(&p).expect("serialize");
    assert_eq!(
        json,
        format!(
            r#"{{"commit_sha":"{}","parent_sha":"{}","fork_sha":"{}","message":"m","resets_status":true}}"#,
            sha("a"),
            sha("b"),
            sha("c")
        )
    );
}

#[test]
fn lifecycle_skips_absent_fields_under_flatten() {
    let entry = LogEntry {
        change_number: ChangeNumber::new(1),
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

#[test]
fn a_comment_logged_before_the_anchor_still_reads() {
    // The five loose fields an entry was written with, and the reply that
    // marked itself by leaving `side` unset.
    let opening = r#"{"thread_id":1,"revision":0,"file":"a.rs","line":3,"side":"new",
        "range":null,"line_text":"x","body":"look","resolved":null}"#;
    let reply = r#"{"thread_id":1,"revision":null,"file":null,"line":null,"side":null,
        "range":null,"line_text":null,"body":"ok","resolved":true}"#;

    let opening: CommentInput = serde_json::from_str(opening).expect("deserialize");
    assert_eq!(
        opening.anchor,
        Some(Anchor::Line {
            file: "a.rs".to_string(),
            side: Side::New,
            line_text: Some("x".to_string()),
            at: LineAnchor::Whole(3),
        })
    );

    let reply: CommentInput = serde_json::from_str(reply).expect("deserialize");
    assert_eq!(reply.anchor, None);
    assert_eq!(reply.resolved, Some(true));

    // An older entry spelled a selection as the range plus its end line.
    let ranged = r#"{"thread_id":2,"revision":0,"file":"a.rs","line":3,"side":"new",
        "range":{"start_line":3,"start_char":1,"end_line":3,"end_char":4},
        "line_text":"x","body":"this","resolved":null}"#;
    let ranged: CommentInput = serde_json::from_str(ranged).expect("deserialize");
    let at = match ranged.anchor {
        Some(Anchor::Line { at, .. }) => Some(at),
        _ => None,
    };
    assert_eq!(
        at,
        Some(LineAnchor::Selection(
            CommentRange::new(3, 1, 3, 4).expect("a forward selection")
        ))
    );
}

#[test]
fn an_anchor_logged_before_the_line_anchor_still_reads() {
    // The spelling an entry was written with when a line anchor held its
    // line and its range side by side.
    let whole = r#"{"line":{"file":"a.rs","side":"new","line":3,
        "line_text":"x","range":null}}"#;
    let selection = r#"{"line":{"file":"a.rs","side":"old","line":4,
        "line_text":"y","range":{"start_line":4,"start_char":1,"end_line":4,"end_char":6}}}"#;

    let whole: Anchor = serde_json::from_str(whole).expect("deserialize");
    assert_eq!(
        whole,
        Anchor::Line {
            file: "a.rs".to_string(),
            side: Side::New,
            line_text: Some("x".to_string()),
            at: LineAnchor::Whole(3),
        }
    );

    let selection: Anchor = serde_json::from_str(selection).expect("deserialize");
    assert_eq!(
        selection,
        Anchor::Line {
            file: "a.rs".to_string(),
            side: Side::Old,
            line_text: Some("y".to_string()),
            at: LineAnchor::Selection(CommentRange::new(4, 1, 4, 6).expect("a forward selection")),
        }
    );
}
