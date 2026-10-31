//! Uploading attachments and putting them on revisions.

mod common;

use common::{
    GitRepo, TestServer, a_change, abandon, by_path, change_log, collect, collect_attachments,
    http_get, http_get_bytes, http_post, http_put_bytes, msg, nit,
};
use jiff::{SignedDuration, Timestamp};
use nit::api::Collected;
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
    assert_eq!(attach_entries(&server, change_number), Vec::<Value>::new());
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

    let collected = collect_attachments(&server, later());

    assert_eq!(collected, Collected { files: 1, bytes: 6 });
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
    std::fs::write(&rowless, b"rowless file").expect("rowless");

    let collected = collect_attachments(&server, later());

    assert_eq!(
        collected,
        Collected {
            files: 2,
            bytes: 19
        },
        "a temp file counts as an attachment"
    );
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

fn amend(server: &TestServer, g: &GitRepo) {
    let amended = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "b\n")]);
    g.branch("feat", amended);
    a_change(server, g, "feat");
}

/// A later revision shows an attachment until it holds another one of the
/// same name. Attaching what a revision already shows appends nothing.
#[test]
fn attachments_carry_forward_by_name() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let plan = upload(&server, b"plan r0");
    let shot = upload(&server, b"shot r0");
    attach(&server, change_number, 0, "plan.md", &plan);
    attach(&server, change_number, 0, "shot.png", &shot);
    amend(&server, &g);
    let replan = upload(&server, b"plan r1");
    attach(&server, change_number, 1, "plan.md", &replan);
    attach(&server, change_number, 1, "shot.png", &shot);

    let (st, detail) = http_get(&server.url(&format!("/api/changes/{change_number}")));

    assert_eq!(st, 200, "{detail}");
    assert_eq!(
        detail["revisions"][1]["attachments"],
        json!([
            { "name": "plan.md", "hash": replan, "revision": 1 },
            { "name": "shot.png", "hash": shot, "revision": 0 },
        ])
    );
    assert_eq!(attach_entries(&server, change_number).len(), 3);
}

fn comment_on_line(server: &TestServer, change_number: u64, file: &str, line: u64) {
    let (st, thread) = http_post(
        &server.url(&format!("/api/changes/{change_number}/comments")),
        &json!({
            "body": "here",
            "revision": 0,
            "anchor": { "line": { "file": file, "side": "new", "at": { "whole": line } } },
        }),
    );
    assert_eq!(st, 200, "{thread}");
}

fn ported_anchors(server: &TestServer, change_number: u64, revision: u64) -> Vec<Value> {
    let (st, ported) = http_get(&server.url(&format!(
        "/api/changes/{change_number}/revisions/{revision}/ported"
    )));
    assert_eq!(st, 200, "{ported}");
    ported
        .as_array()
        .expect("ported")
        .iter()
        .map(|p| p["anchor"].clone())
        .collect()
}

/// A thread on a text attachment shifts through the edits between the
/// versions that two revisions show.
#[test]
fn a_thread_on_an_attachment_ports_through_its_edits() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let before = upload(&server, b"a\nb\nc\n");
    attach(&server, change_number, 0, "notes.txt", &before);
    comment_on_line(&server, change_number, "/attachments/notes.txt", 2);
    amend(&server, &g);
    let after = upload(&server, b"new\na\nb\nc\n");
    attach(&server, change_number, 1, "notes.txt", &after);

    assert_eq!(
        ported_anchors(&server, change_number, 1),
        [
            json!({ "line": { "file": "/attachments/notes.txt", "side": "new", "at": { "whole": 3 } } })
        ]
    );
}

/// A thread on an attachment that carries forward keeps its line.
#[test]
fn a_thread_on_a_carried_attachment_keeps_its_line() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let notes = upload(&server, b"a\nb\n");
    attach(&server, change_number, 0, "notes.txt", &notes);
    comment_on_line(&server, change_number, "/attachments/notes.txt", 2);
    amend(&server, &g);

    assert_eq!(
        ported_anchors(&server, change_number, 1),
        [
            json!({ "line": { "file": "/attachments/notes.txt", "side": "new", "at": { "whole": 2 } } })
        ]
    );
}

/// The file at `path` of the diff of `revision`, with `query` appended.
fn diff_file(
    server: &TestServer,
    change_number: u64,
    revision: u64,
    query: &str,
    path: &str,
) -> Value {
    let (st, diff) = http_get(&server.url(&format!(
        "/api/changes/{change_number}/revisions/{revision}/diff{query}"
    )));
    assert_eq!(st, 200, "{diff}");
    by_path(&diff, path)
}

/// Each line of a diff file, as its kind, old number, new number and text.
fn lines(file: &Value) -> Vec<(String, Value, Value, String)> {
    file["hunks"]
        .as_array()
        .expect("hunks")
        .iter()
        .flat_map(|h| h["lines"].as_array().expect("lines").clone())
        .map(|l| {
            (
                l["kind"].as_str().expect("kind").to_string(),
                l["old"].clone(),
                l["new"].clone(),
                l["text"].as_str().expect("text").to_string(),
            )
        })
        .collect()
}

/// Against its parent, a revision's diff shows each attachment as an
/// added file, and an attachment that is not text as a binary one.
#[test]
fn the_diff_shows_the_attachments_as_added_files() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let notes = upload(&server, b"a\nb\n");
    let shot = upload(&server, b"\x89PNG\x00\xff");
    attach(&server, change_number, 0, "notes.txt", &notes);
    attach(&server, change_number, 0, "shot.png", &shot);

    let notes = diff_file(&server, change_number, 0, "", "/attachments/notes.txt");
    let shot = diff_file(&server, change_number, 0, "", "/attachments/shot.png");

    assert_eq!(notes["status"], "added");
    assert_eq!(
        lines(&notes),
        [
            ("add".into(), Value::Null, json!(1), "a".into()),
            ("add".into(), Value::Null, json!(2), "b".into()),
        ]
    );
    assert_eq!(shot["binary"], true);
    assert_eq!(shot["hunks"], json!([]));
}

/// An interdiff compares the attachment that each revision shows. An
/// attachment that carries forward unchanged stays out of it.
#[test]
fn an_interdiff_compares_the_attachments_of_the_two_revisions() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let before = upload(&server, b"a\nb\n");
    let kept = upload(&server, b"kept\n");
    attach(&server, change_number, 0, "notes.txt", &before);
    attach(&server, change_number, 0, "kept.txt", &kept);
    amend(&server, &g);
    let after = upload(&server, b"a\nB\n");
    attach(&server, change_number, 1, "notes.txt", &after);

    let (st, diff) = http_get(&server.url(&format!(
        "/api/changes/{change_number}/revisions/1/diff?against=0"
    )));

    assert_eq!(st, 200, "{diff}");
    let notes = by_path(&diff, "/attachments/notes.txt");
    assert_eq!(notes["status"], "modified");
    assert_eq!(
        lines(&notes),
        [
            ("context".into(), json!(1), json!(1), "a".into()),
            ("del".into(), json!(2), Value::Null, "b".into()),
            ("add".into(), Value::Null, json!(2), "B".into()),
        ]
    );
    assert!(
        diff["files"]
            .as_array()
            .expect("files")
            .iter()
            .all(|f| f["path"] != "/attachments/kept.txt"),
        "{diff}"
    );
}

/// `/lines` serves the whole attachment, as the interdiff compares it.
#[test]
fn lines_of_an_attachment() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let before = upload(&server, b"a\nb\nc\nd\ne\nf\ng\nh\n");
    attach(&server, change_number, 0, "notes.txt", &before);
    amend(&server, &g);
    let after = upload(&server, b"a\nb\nc\nd\ne\nf\ng\nH\n");
    attach(&server, change_number, 1, "notes.txt", &after);

    let (st, whole) = http_get(&server.url(&format!(
        "/api/changes/{change_number}/revisions/1/lines?path=/attachments/notes.txt&against=0"
    )));

    assert_eq!(st, 200, "{whole}");
    let texts: Vec<&str> = whole["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|l| l["text"].as_str().expect("text"))
        .collect();
    assert_eq!(texts, ["a", "b", "c", "d", "e", "f", "g", "h", "H"]);
}

/// `nit attach` uploads a file and attaches it to the latest revision,
/// named by its path from the repository root.
#[test]
fn nit_attach_names_the_file_by_its_path_from_the_root() {
    let g = GitRepo::new();
    let (server, change_number) = server_with_change(&g);
    let dir = g.workdir().join("web/screenshots");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("review.png"), b"png").expect("file");
    let change = change_number.to_string();

    let (ok, _, stderr) = nit(
        &server,
        &g,
        &["attach", "web/screenshots/review.png", "--change", &change],
    );

    assert!(ok, "{stderr}");
    assert_eq!(
        attach_entries(&server, change_number),
        [json!({
            "revision": 0,
            "name": "web/screenshots/review.png",
            "hash": blake3::hash(b"png").to_hex().to_string(),
        })]
    );
}

/// `nit attach` needs the change named.
#[test]
fn nit_attach_requires_the_change() {
    let g = GitRepo::new();
    let (server, _) = server_with_change(&g);
    std::fs::write(g.workdir().join("out.txt"), b"x").expect("file");

    let (ok, _, _) = nit(&server, &g, &["attach", "out.txt"]);

    assert!(!ok);
}
