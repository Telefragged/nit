//! Uploading attachments and putting them on revisions.

mod common;

use common::{GitRepo, TestServer, http_put_bytes};

fn server(g: &GitRepo) -> TestServer {
    TestServer::start(g.dir.path().join("nit.sqlite3"), None)
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
