mod common;

use common::{
    GitRepo, TestServer, collect_attachments, http_get, http_get_bytes, http_post, http_post_bytes,
    http_put_bytes, post, replica, ws_read, ws_read_binary, ws_subscribe_at,
};
use jiff::{SignedDuration, Timestamp};
use nit_document::{Document, Update, Version, Writer};
use serde_json::{Value, json};

fn server_with_document(g: &GitRepo, tags: &[(&str, &str)]) -> (TestServer, u64) {
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let tags: serde_json::Map<String, Value> = tags
        .iter()
        .map(|(key, value)| ((*key).to_owned(), json!(value)))
        .collect();
    let (st, document) = http_post(
        &server.url("/api/documents"),
        &json!({ "title": "Plan", "tags": tags }),
    );
    assert_eq!(st, 200, "{document}");
    let number = document["number"].as_u64().expect("number");
    (server, number)
}

fn write(document: &mut Document, writer: Writer, text: &str) {
    document.editor(writer).splice(0, 0, text).unwrap();
}

fn detail(server: &TestServer, number: u64) -> Value {
    let (st, detail) = http_get(&server.url(&format!("/api/documents/{number}")));
    assert_eq!(st, 200, "{detail}");
    detail
}

fn numbers(server: &TestServer, query: &str) -> Vec<u64> {
    let (st, list) = http_get(&server.url(&format!("/api/documents{query}")));
    assert_eq!(st, 200, "{list}");
    list["documents"]
        .as_array()
        .expect("documents")
        .iter()
        .map(|d| d["number"].as_u64().expect("number"))
        .collect()
}

#[test]
fn concurrent_updates_of_two_clients_merge() {
    let g = GitRepo::new();
    let (server, number) = server_with_document(&g, &[]);
    let mut author = replica(&server, number);
    let mut reviewer = replica(&server, number);
    let start = author.version();
    write(&mut author, Writer::Author, "from the author\n");
    write(&mut reviewer, Writer::Reviewer, "from the reviewer\n");

    let (st, res) = post(&server, number, &author, &start);
    assert_eq!(st, 200, "{res}");
    let (st, res) = post(&server, number, &reviewer, &start);
    assert_eq!(st, 200, "{res}");

    let merged = replica(&server, number);
    let text = merged.text();
    assert!(text.contains("from the author\n") && text.contains("from the reviewer\n"));
    for client in [&mut author, &mut reviewer] {
        client.merge(&merged.updates_since(&start)).unwrap();
        assert_eq!(client.text(), text);
    }
    let stored = detail(&server, number);
    assert_eq!(stored["version"], json!(merged.version()));
    let (st, again) = post(&server, number, &author, &start);
    assert_eq!(st, 200, "{again}");
    assert_eq!(detail(&server, number), stored);
}

#[test]
fn bytes_that_are_not_an_update_are_a_400() {
    let g = GitRepo::new();
    let (server, number) = server_with_document(&g, &[]);
    let before = detail(&server, number);
    let (st, res) = http_post_bytes(
        &server.url(&format!("/api/documents/{number}/updates")),
        b"not an update",
    );
    assert_eq!(st, 400, "{res}");
    assert_eq!(detail(&server, number), before);
    let (st, _) = http_post_bytes(&server.url("/api/documents/999/updates"), b"");
    assert_eq!(st, 404);
}

#[test]
fn the_list_selects_by_tag() {
    let g = GitRepo::new();
    let (server, number) = server_with_document(&g, &[("session-id", "s1")]);
    assert_eq!(numbers(&server, ""), [number]);
    assert_eq!(numbers(&server, "?tag=session-id=s1"), [number]);
    assert_eq!(numbers(&server, "?tag=session-id=s2"), [0u64; 0]);
}

#[test]
fn the_stream_sends_the_updates_that_a_client_lacks() {
    let g = GitRepo::new();
    let (server, number) = server_with_document(&g, &[]);
    let mut author = replica(&server, number);
    let mut reviewer = replica(&server, number);
    let start = author.version();
    let mut socket = ws_subscribe_at(
        &server,
        "/api/documents/stream",
        &json!({ "updates": { number.to_string(): start } }),
    );

    write(&mut author, Writer::Author, "one\n");
    let (st, res) = post(&server, number, &author, &start);
    assert_eq!(st, 200, "{res}");
    let frame = ws_read_binary(&mut socket);
    let (document, update) = frame.split_first_chunk::<8>().expect("a document number");
    assert_eq!(u64::from_le_bytes(*document), number);
    reviewer
        .merge(&Update::from_bytes(update.to_vec()))
        .unwrap();
    assert_eq!(reviewer.text(), author.text());

    let first = author.version();
    write(&mut author, Writer::Author, "two\n");
    let (st, res) = post(&server, number, &author, &first);
    assert_eq!(st, 200, "{res}");
    let frame = ws_read_binary(&mut socket);
    assert_eq!(&frame[8..], author.updates_since(&first).as_bytes());
}

#[test]
fn the_stream_reports_a_sent_version() {
    let g = GitRepo::new();
    let (server, number) = server_with_document(&g, &[("session-id", "s1")]);
    let mut reviewer = replica(&server, number);
    let start = reviewer.version();
    write(&mut reviewer, Writer::Reviewer, "a note\n");
    let send = |version: &Version| {
        http_post(
            &server.url(&format!("/api/documents/{number}/send")),
            &json!(version),
        )
    };
    let (st, res) = send(&reviewer.version());
    assert_eq!(st, 400, "the server lacks the edit: {res}");

    let (st, res) = post(&server, number, &reviewer, &start);
    assert_eq!(st, 200, "{res}");
    let mut socket = ws_subscribe_at(
        &server,
        "/api/documents/stream",
        &json!({ "sent": { "tag": ["session-id=s1"] } }),
    );
    let (st, res) = send(&reviewer.version());
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["sent"], json!(reviewer.version()));
    let frame = ws_read(&mut socket);
    assert_eq!(
        frame,
        json!({ "document": number, "version": reviewer.version() })
    );
}

#[test]
fn a_referenced_upload_stays_through_a_collection() {
    let g = GitRepo::new();
    let (server, number) = server_with_document(&g, &[]);
    let (st, uploaded) = http_put_bytes(&server.url("/api/attachments"), b"picture");
    assert_eq!(st, 200, "{uploaded}");
    let hash = uploaded["hash"].as_str().expect("hash").to_owned();
    let upload = |hash: &str| format!("![picture](/api/attachments/{hash}/picture.png)\n");

    let mut author = replica(&server, number);
    let start = author.version();
    write(&mut author, Writer::Author, &upload(&"0".repeat(64)));
    let (st, res) = post(&server, number, &author, &start);
    assert_eq!(st, 404, "{res}");

    let mut author = replica(&server, number);
    write(&mut author, Writer::Author, &upload(&hash));
    let (st, res) = post(&server, number, &author, &start);
    assert_eq!(st, 200, "{res}");
    collect_attachments(&server, Timestamp::now() + SignedDuration::from_hours(1));
    let (st, _, bytes) =
        http_get_bytes(&server.url(&format!("/api/attachments/{hash}/picture.png")));
    assert_eq!((st, bytes.as_slice()), (200, &b"picture"[..]));
}
