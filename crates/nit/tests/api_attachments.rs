//! Uploading attachments and putting them on revisions.

mod common;

use common::{
    GitRepo, TestServer, a_change, abandon, change_log, collect, collect_attachments,
    http_get_bytes, http_post, http_put_bytes, msg,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

fn server(g: &GitRepo) -> TestServer {
    TestServer::start(g.dir.path().join("nit.sqlite3"), None)
}

/// A server with one change of one revision, and that change's number.
fn server_with_change(g: &GitRepo) -> (TestServer, u64) {
    let a = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", a);
    let server = server(g);
    let change_number = a_change(&server, g, "feat");
    (server, change_number)
}

fn upload(server: &TestServer, body: &[u8]) -> String {
    let (st, uploaded) = http_put_bytes(&server.url("/api/attachments"), body);
    assert_eq!(st, 200, "{uploaded}");
    uploaded["hash"].as_str().expect("hash").to_string()
}

fn attach(
    server: &TestServer,
    change_number: u64,
    revision: u64,
    name: &str,
    hash: &str,
) -> (u16, Value) {
    http_post(
        &server.url(&format!("/api/changes/{change_number}/attachments")),
        &json!({ "revision": revision, "name": name, "hash": hash }),
    )
}

fn attach_entries(server: &TestServer, change_number: u64) -> Vec<Value> {
    change_log(server, change_number)
        .into_iter()
        .filter(|e| e["kind"] == "attach")
        .map(|e| e["payload"].clone())
        .collect()
}

/// An upload answers the BLAKE3 hash of the body and stores the bytes
/// under it, beside the database.
#[test]
fn upload_stores_the_body_under_its_hash() {
    let g = GitRepo::new();
    let server = server(&g);
    let body = b"# Plan\n";

    let (st, uploaded) = http_put_bytes(&server.url("/api/attachments"), body);

    assert_eq!(st, 200, "{uploaded}");
    let hash = blake3::hash(body).to_hex().to_string();
    assert_eq!(uploaded["hash"], hash.as_str());
    let stored = std::fs::read(g.dir.path().join("attachments").join(&hash)).expect("blob");
    assert_eq!(stored, body);
}

/// Uploading bytes the store holds again succeeds with the same hash.
#[test]
fn upload_of_the_same_bytes_is_idempotent() {
    let g = GitRepo::new();
    let server = server(&g);

    let (_, first) = http_put_bytes(&server.url("/api/attachments"), b"same");
    let (st, second) = http_put_bytes(&server.url("/api/attachments"), b"same");

    assert_eq!(st, 200, "{second}");
    assert_eq!(first["hash"], second["hash"]);
}

/// A body over the size limit is refused, and leaves no file behind.
#[test]
fn upload_over_the_limit_is_413() {
    let g = GitRepo::new();
    let server = server(&g);
    let body = vec![0u8; 64 * 1024 * 1024 + 1];

    let (st, _) = http_put_bytes(&server.url("/api/attachments"), &body);

    assert_eq!(st, 413);
    let store = g.dir.path().join("attachments");
    assert_eq!(
        std::fs::read_dir(store.join(".tmp")).expect("tmp").count(),
        0
    );
    let blobs = std::fs::read_dir(&store)
        .expect("store")
        .filter(|e| e.as_ref().expect("entry").file_name() != ".tmp")
        .count();
    assert_eq!(blobs, 0);
}

/// An attach appends an entry naming the revision, the name and the hash.
#[test]
fn attach_appends_an_attach_entry() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let hash = upload(&server, b"# Plan\n");

    let (st, res) = attach(&server, change_number, 0, "plan/attachments.md", &hash);

    assert_eq!(st, 200, "{res}");
    assert_eq!(
        attach_entries(&server, change_number),
        [json!({ "revision": 0, "name": "plan/attachments.md", "hash": hash })]
    );
}

/// Attaching the name and hash a revision already holds appends nothing.
/// Attaching other bytes under that name appends a replacement.
#[test]
fn attach_of_the_held_name_and_hash_appends_nothing() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let first = upload(&server, b"one");
    let second = upload(&server, b"two");

    attach(&server, change_number, 0, "out.txt", &first);
    let (st, res) = attach(&server, change_number, 0, "out.txt", &first);
    assert_eq!(st, 200, "{res}");
    attach(&server, change_number, 0, "out.txt", &second);

    let hashes: Vec<Value> = attach_entries(&server, change_number)
        .into_iter()
        .map(|p| p["hash"].clone())
        .collect();
    assert_eq!(hashes, [json!(first), json!(second)]);
}

/// A hash the store never received is a 404.
#[test]
fn attach_of_an_unknown_blob_is_404() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);

    let (st, res) = attach(&server, change_number, 0, "out.txt", &"0".repeat(64));

    assert_eq!(st, 404, "{res}");
    assert!(attach_entries(&server, change_number).is_empty());
}

/// A revision the change does not have is a 404.
#[test]
fn attach_to_a_missing_revision_is_404() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let hash = upload(&server, b"x");

    let (st, res) = attach(&server, change_number, 1, "out.txt", &hash);

    assert_eq!(st, 404, "{res}");
}

/// A name that is not a relative path is a 400.
#[test]
fn attach_under_an_absolute_name_is_400() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let hash = upload(&server, b"x");

    let (st, res) = attach(&server, change_number, 0, "/etc/passwd", &hash);

    assert_eq!(st, 400, "{res}");
}

/// A download returns the bytes with a content type from the name, and with
/// a sandbox policy for HTML pages.
#[test]
fn download_serves_the_bytes_typed_by_the_name() {
    let g = GitRepo::new();
    let server = server(&g);
    let hash = upload(&server, b"<p>hi</p>");

    let (st, headers, body) =
        http_get_bytes(&server.url(&format!("/api/attachments/{hash}/page.html")));

    assert_eq!(st, 200);
    assert_eq!(body, b"<p>hi</p>");
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert_eq!(headers["content-security-policy"], "sandbox allow-scripts");
    assert_eq!(headers["x-content-type-options"], "nosniff");
}

/// A hash the store does not hold is a 404.
#[test]
fn download_of_an_unknown_blob_is_404() {
    let g = GitRepo::new();
    let server = server(&g);

    let (st, _, _) =
        http_get_bytes(&server.url(&format!("/api/attachments/{}/a.png", "0".repeat(64))));

    assert_eq!(st, 404);
}

fn later() -> Timestamp {
    Timestamp::now() + SignedDuration::from_hours(1)
}

fn stored(g: &GitRepo, hash: &str) -> bool {
    g.dir.path().join("attachments").join(hash).exists()
}

/// A collection deletes a blob that no change links to, and a later attach
/// of it is a 404.
#[test]
fn collect_deletes_an_unheld_blob() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let hash = upload(&server, b"orphan");

    collect_attachments(&server, later());

    assert!(!stored(&g, &hash));
    let (st, res) = attach(&server, change_number, 0, "out.txt", &hash);
    assert_eq!(st, 404, "{res}");
}

/// A blob uploaded after the cutoff stays.
#[test]
fn collect_keeps_a_blob_newer_than_the_cutoff() {
    let g = GitRepo::new();
    let server = server(&g);
    let cutoff = Timestamp::now();
    let hash = upload(&server, b"fresh");

    collect_attachments(&server, cutoff);

    assert!(stored(&g, &hash));
}

/// A blob that a change links to stays until the change is collected.
#[test]
fn collect_frees_a_blob_with_its_change() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let hash = upload(&server, b"held");
    attach(&server, change_number, 0, "out.txt", &hash);

    collect_attachments(&server, later());
    assert!(stored(&g, &hash), "the open change holds the blob");

    abandon(&server, change_number);
    collect(&server, later());
    collect_attachments(&server, later());
    assert!(!stored(&g, &hash), "the collected change freed the blob");
}

/// A collection deletes a temp file and a blob file that has no row.
#[test]
fn collect_deletes_stray_files() {
    let g = GitRepo::new();
    let server = server(&g);
    let store = g.dir.path().join("attachments");
    let temp = store.join(".tmp").join("stray");
    let rowless = store.join("1".repeat(64));
    std::fs::write(&temp, b"partial").expect("temp");
    std::fs::write(&rowless, b"rowless").expect("rowless");

    collect_attachments(&server, later());

    assert!(!temp.exists());
    assert!(!rowless.exists());
}

/// A comment on a line of a text attachment snapshots that line.
#[test]
fn comment_on_an_attachment_line_snapshots_its_text() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let text = upload(&server, b"first\nsecond\n");
    attach(&server, change_number, 0, "notes.txt", &text);

    let (st, thread) = http_post(
        &server.url(&format!("/api/changes/{change_number}/comments")),
        &json!({
            "body": "why?",
            "revision": 0,
            "anchor": { "line": { "file": "/attachments/notes.txt", "side": "new", "at": { "whole": 2 } } },
        }),
    );

    assert_eq!(st, 200, "{thread}");
    assert_eq!(thread["anchor"]["line"]["line_text"], "second");
}
