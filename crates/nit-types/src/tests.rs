//! Serde round-trip tests, run by the `test-nit-types` flake check with no
//! optional features — exercising the no-feature baseline that an optional
//! feature (clap, ts) would otherwise mask.

use crate::domain::ChangeNumber;
use crate::domain::TagsPayload;
use crate::domain::{CommentInput, CommentRange, CommentTarget, FileAnchor, LogAnchor};
use crate::domain::{LifecycleAction, RevisionNumber, Side};
use crate::domain::{LifecyclePayload, LogEntry, LogPayload, RevisionPayload};
use crate::domain::{Sha, Tag, Tags};

const COMMIT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const PARENT: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const FORK: &str = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";

fn revision_entry() -> LogEntry {
    LogEntry {
        change_number: ChangeNumber::new(7),
        position: 2,
        created_at: "t".to_string(),
        payload: LogPayload::Revision(RevisionPayload {
            commit_sha: Sha::new(COMMIT).expect("valid sha"),
            parent_sha: Sha::new(PARENT).expect("valid sha"),
            fork_sha: Sha::new(FORK).expect("valid sha"),
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
            r#"{{"change_number":7,"position":2,"created_at":"t","kind":"revision","payload":{{"commit_sha":"{COMMIT}","parent_sha":"{PARENT}","fork_sha":"{FORK}","message":"m","resets_status":true}}}}"#
        )
    );
}

#[test]
fn log_entry_round_trips() {
    let json = serde_json::to_string(&revision_entry()).expect("serialize");
    let back: LogEntry = serde_json::from_str(&json).expect("deserialize");
    match back.payload {
        LogPayload::Revision(p) => assert_eq!(p.commit_sha, Sha::new(COMMIT).expect("valid sha")),
        _ => panic!("expected a revision payload"),
    }
}

#[test]
fn payload_serializes_as_the_bare_inner_struct() {
    // The storage boundary serializes the inner struct alone (kind goes in its
    // own column).
    let p = RevisionPayload {
        commit_sha: Sha::new(COMMIT).expect("valid sha"),
        parent_sha: Sha::new(PARENT).expect("valid sha"),
        fork_sha: Sha::new(FORK).expect("valid sha"),
        message: "m".to_string(),
        resets_status: true,
    };
    let json = serde_json::to_string(&p).expect("serialize");
    assert_eq!(
        json,
        format!(
            r#"{{"commit_sha":"{COMMIT}","parent_sha":"{PARENT}","fork_sha":"{FORK}","message":"m","resets_status":true}}"#
        )
    );
}

#[test]
fn a_tags_payload_round_trips() {
    let p = TagsPayload {
        tags: [Tag::new("branch", "track/tags").expect("valid tag")]
            .into_iter()
            .collect::<Tags>(),
    };
    let json = serde_json::to_string(&p).expect("serialize");
    assert_eq!(json, r#"{"tags":{"branch":"track/tags"}}"#);
    let back: TagsPayload = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.tags, p.tags);
}

#[test]
fn a_tag_outside_the_vocabulary_fails_to_deserialize() {
    assert!(serde_json::from_str::<TagsPayload>(r#"{"tags":{"-bad":"v"}}"#).is_err());
}

#[test]
fn lifecycle_skips_absent_fields_under_flatten() {
    let entry = LogEntry {
        change_number: ChangeNumber::new(1),
        position: 0,
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
        r#"{"change_number":1,"position":0,"created_at":"t","kind":"lifecycle","payload":{"action":"merged"}}"#
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
fn subscription_spells_its_query_like_the_change_list() {
    use crate::changes::ChangeQuery;
    use crate::domain::Tag;
    use crate::events::Subscription;
    let msg = Subscription {
        query: ChangeQuery {
            repo: Some(1),
            tag: vec![Tag::new("branch", "feat").expect("valid tag")],
            ..ChangeQuery::default()
        },
        positions: [(crate::domain::ChangeNumber::new(7), 3)].into(),
        start: crate::events::Start::Log,
    };
    let json = serde_json::to_string(&msg).expect("serialize");
    assert_eq!(
        json,
        r#"{"query":{"repo":1,"tag":["branch=feat"]},"positions":{"7":3},"start":"log"}"#
    );
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
        *opening.target(),
        CommentTarget::Open {
            thread_id: Some(1),
            revision: RevisionNumber::new(0),
            anchor: LogAnchor::File {
                path: "a.rs".to_string(),
                side: Side::New,
                anchor: FileAnchor::Line(3),
            },
        }
    );

    let reply: CommentInput = serde_json::from_str(reply).expect("deserialize");
    assert_eq!(*reply.target(), CommentTarget::Reply { thread_id: 1 });
    assert_eq!(reply.resolved(), Some(true));

    // An older entry spelled a selection as the range plus its end line.
    let ranged = r#"{"thread_id":2,"revision":0,"file":"a.rs","line":3,"side":"new",
        "range":{"start_line":3,"start_char":1,"end_line":3,"end_char":4},
        "line_text":"x","body":"this","resolved":null}"#;
    let ranged: CommentInput = serde_json::from_str(ranged).expect("deserialize");
    let anchor = match *ranged.target() {
        CommentTarget::Open {
            anchor: LogAnchor::File { anchor, .. },
            ..
        } => Some(anchor),
        _ => None,
    };
    assert_eq!(
        anchor,
        Some(FileAnchor::Range(
            CommentRange::new(3, 1, 3, 4).expect("a forward selection")
        ))
    );
}

#[test]
fn an_anchor_logged_before_the_file_anchor_still_reads() {
    let read = |json: &str| serde_json::from_str::<LogAnchor>(json).expect("deserialize");
    let file = |side, anchor| LogAnchor::File {
        path: "a.rs".to_string(),
        side,
        anchor,
    };
    let range = CommentRange::new(4, 1, 4, 6).expect("a forward selection");

    assert_eq!(
        read(r#"{"file":{"file":"a.rs"}}"#),
        file(Side::New, FileAnchor::File)
    );

    assert_eq!(
        read(r#"{"line":{"file":"a.rs","side":"new","line_text":"y","at":{"whole":3}}}"#),
        file(Side::New, FileAnchor::Line(3))
    );
    assert_eq!(
        read(
            r#"{"line":{"file":"a.rs","side":"old","line_text":"y",
            "at":{"selection":{"start_line":4,"start_char":1,"end_line":4,"end_char":6}}}}"#
        ),
        file(Side::Old, FileAnchor::Range(range))
    );

    assert_eq!(
        read(r#"{"line":{"file":"a.rs","side":"new","line":3,"line_text":"y","range":null}}"#),
        file(Side::New, FileAnchor::Line(3))
    );
    assert_eq!(
        read(
            r#"{"line":{"file":"a.rs","side":"old","line":4,"line_text":"y",
            "range":{"start_line":4,"start_char":1,"end_line":4,"end_char":6}}}"#
        ),
        file(Side::Old, FileAnchor::Range(range))
    );

    let current = file(Side::Old, FileAnchor::File);
    assert_eq!(
        read(&serde_json::to_string(&current).expect("serialize")),
        current
    );
}

#[test]
fn a_comment_without_a_target_does_not_parse() {
    let opening = r#"{"thread_id":null,"revision":null,"anchor":"change","body":"x"}"#;
    let orphan = r#"{"thread_id":null,"anchor":null,"body":"x"}"#;
    assert!(serde_json::from_str::<CommentInput>(opening).is_err());
    assert!(serde_json::from_str::<CommentInput>(orphan).is_err());
}

#[test]
fn an_empty_comment_does_not_parse() {
    let opening = r#"{"thread_id":null,"revision":0,"anchor":"change","body":" "}"#;
    let reply = r#"{"thread_id":1,"body":"","resolved":null}"#;
    assert!(serde_json::from_str::<CommentInput>(opening).is_err());
    assert!(serde_json::from_str::<CommentInput>(reply).is_err());
}
