//! The diff endpoint: `/COMMIT_MSG` leads every response, real files
//! carry exact add/del counts and hunk lines, binary files have empty
//! hunks, `?mode=outline` lists only the files with an outline to show, and
//! `?against` produces an interdiff whose `/COMMIT_MSG` is a real message
//! diff. Revisions are minted by `push` — amend + re-push
//! gives revision 1.

mod common;

use common::{GitRepo, TestServer, change_id, http_get, msg, push};
use serde_json::{Value, json};

fn lines(prefix: &str, n: std::ops::RangeInclusive<i64>) -> String {
    use std::fmt::Write;
    n.fold(String::new(), |mut s, i| {
        writeln!(s, "{prefix}{i}").unwrap();
        s
    })
}

/// Revision 0 of the tip change lives here after the first push.
fn tip_change_number(push_result: &Value) -> u64 {
    push_result["tip_change"]["change_number"]
        .as_u64()
        .expect("a tip change")
}

/// Look a file up by its new path in a `Diff`.
fn by_path(diff: &Value, p: &str) -> Value {
    diff["files"]
        .as_array()
        .expect("files array")
        .iter()
        .find(|f| f["path"] == p)
        .unwrap_or_else(|| panic!("no file {p} in {diff}"))
        .clone()
}

/// A revision-0 diff against parent: `/COMMIT_MSG` leads (status added), a
/// multi-hunk modification, an added file with exact line numbers, and a
/// binary modification flagged with empty hunks.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one diff shape asserted exhaustively"
)]
fn diff_vs_parent_leads_with_commit_msg() {
    let g = GitRepo::new();
    let keep_v1 = lines("k", 1..=12);
    let base = g.commit_full(
        &[g.root],
        "base files\n",
        &[
            ("keep.txt", keep_v1.as_bytes()),
            ("data.bin", b"\x00\x01\x02binary-one\n"),
        ],
        &[],
    );
    g.branch("main", base);

    // Far-apart edits (k2 and k11) force two separate hunks — context windows
    // don't overlap.
    let keep_v2 = keep_v1
        .replace("k2\n", "k2 changed\n")
        .replace("k11\n", "k11 changed\n");
    let c1 = g.commit_full(
        &[base],
        &msg("feat: diff", "Idiff0001"),
        &[
            ("keep.txt", keep_v2.as_bytes()),
            ("data.bin", b"\x00\x01\x02binary-two\n"),
            ("fresh.txt", b"hello\nworld\n"),
        ],
        &[],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_number(&pushed);
    assert_eq!(pushed["tip_change"]["revision"], 0, "first revision is 0");

    let (st, diff) = http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff")));
    assert_eq!(st, 200, "{diff}");

    // The exact wire shape of the synthetic commit-message file.
    assert_eq!(
        diff["files"][0],
        json!({
            "path": "/COMMIT_MSG",
            "status": "added",
            "binary": false,
            "additions": 3,
            "deletions": 0,
            "new_total": 3,
            "hunks": [{
                "old_start": 0, "old_lines": 0, "new_start": 1, "new_lines": 3,
                "header": "",
                "lines": [
                    {"kind": "add", "new": 1, "text": "feat: diff"},
                    {"kind": "add", "new": 2, "text": ""},
                    {"kind": "add", "new": 3, "text": format!("Change-Id: {}", change_id("Idiff0001"))},
                ],
            }],
        })
    );

    assert_eq!(diff["files"].as_array().unwrap().len(), 4);

    assert_eq!(
        by_path(&diff, "keep.txt"),
        json!({
            "path": "keep.txt",
            "status": "modified",
            "binary": false,
            "additions": 2,
            "deletions": 2,
            "new_total": 12,
            "hunks": [
                {
                    "old_start": 1, "old_lines": 5, "new_start": 1, "new_lines": 5,
                    "header": "",
                    "lines": [
                        {"kind": "context", "old": 1, "new": 1, "text": "k1"},
                        {"kind": "del", "old": 2, "text": "k2"},
                        {"kind": "add", "new": 2, "text": "k2 changed"},
                        {"kind": "context", "old": 3, "new": 3, "text": "k3"},
                        {"kind": "context", "old": 4, "new": 4, "text": "k4"},
                        {"kind": "context", "old": 5, "new": 5, "text": "k5"},
                    ],
                },
                {
                    "old_start": 8, "old_lines": 5, "new_start": 8, "new_lines": 5,
                    "header": "k7",
                    "lines": [
                        {"kind": "context", "old": 8, "new": 8, "text": "k8"},
                        {"kind": "context", "old": 9, "new": 9, "text": "k9"},
                        {"kind": "context", "old": 10, "new": 10, "text": "k10"},
                        {"kind": "del", "old": 11, "text": "k11"},
                        {"kind": "add", "new": 11, "text": "k11 changed"},
                        {"kind": "context", "old": 12, "new": 12, "text": "k12"},
                    ],
                },
            ],
        })
    );

    assert_eq!(
        by_path(&diff, "fresh.txt"),
        json!({
            "path": "fresh.txt",
            "status": "added",
            "binary": false,
            "additions": 2,
            "deletions": 0,
            "new_total": 2,
            "hunks": [{
                "old_start": 0, "old_lines": 0, "new_start": 1, "new_lines": 2,
                "header": "",
                "lines": [
                    {"kind": "add", "new": 1, "text": "hello"},
                    {"kind": "add", "new": 2, "text": "world"},
                ],
            }],
        })
    );

    assert_eq!(
        by_path(&diff, "data.bin"),
        json!({
            "path": "data.bin",
            "status": "modified",
            "binary": true,
            "additions": 0,
            "deletions": 0,
            "new_total": 0,
            "hunks": [],
        })
    );
}

/// `?against={m}` is an interdiff `tree(m) → tree(n)`: its `/COMMIT_MSG` is a
/// real message diff (status modified), and a file the amend touched shows
/// its r0 → r1 delta. Revision 1 is created by amending and re-pushing.
#[test]
fn interdiff_against_earlier_revision() {
    let g = GitRepo::new();
    let body_v1 = lines("b", 1..=8);
    let base = g.commit_full(
        &[g.root],
        "base\n",
        &[("body.txt", body_v1.as_bytes())],
        &[],
    );
    g.branch("main", base);

    let c1 = g.commit(
        &[base],
        &msg("feat: thing", "Iinter01"),
        &[("body.txt", &lines("b", 1..=8).replace("b4\n", "b4 v1\n"))],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_number(&pushed);
    assert_eq!(pushed["tip_change"]["revision"], 0);

    let c2 = g.commit(
        &[base],
        &msg("feat: thing, reworded", "Iinter01"),
        &[("body.txt", &lines("b", 1..=8).replace("b4\n", "b4 v2\n"))],
    );
    g.branch("feat", c2);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    assert_eq!(
        pushed["tip_change"]["revision"], 1,
        "amend mints revision 1"
    );

    let (st, diff) =
        http_get(&server.url(&format!("/api/changes/{id}/revisions/1/diff?against=0")));
    assert_eq!(st, 200, "{diff}");

    // The message file leads and is a MODIFIED message diff (subject reworded),
    // not the all-add vs-parent form.
    let cm = diff["files"][0].clone();
    assert_eq!(cm["path"], "/COMMIT_MSG");
    assert_eq!(cm["status"], "modified");
    assert_eq!(cm["binary"], false);
    assert_eq!(cm["additions"], 1);
    assert_eq!(cm["deletions"], 1);
    let cm_lines = cm["hunks"][0]["lines"].as_array().unwrap();
    let cm_del = cm_lines
        .iter()
        .find(|l| l["kind"] == "del")
        .expect("a deleted subject line");
    assert_eq!(cm_del["text"], "feat: thing");
    let cm_add = cm_lines
        .iter()
        .find(|l| l["kind"] == "add")
        .expect("an added subject line");
    assert_eq!(cm_add["text"], "feat: thing, reworded");

    let body = by_path(&diff, "body.txt");
    assert_eq!(body["status"], "modified");
    assert_eq!(
        (body["additions"].as_i64(), body["deletions"].as_i64()),
        (Some(1), Some(1))
    );
    let del = body["hunks"][0]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["kind"] == "del")
        .expect("the r0 line");
    assert_eq!(del["text"], "b4 v1");
    let add = body["hunks"][0]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["kind"] == "add")
        .expect("the r1 line");
    assert_eq!(add["text"], "b4 v2");
}

/// A revision that was never pushed is a 404 — vs-parent and as an interdiff
/// endpoint alike.
/// `?mode=outline` answers with the files whose outline changed, and with
/// nothing else: only the signature survives, while the body-only edit,
/// the rename and the binary file all go.
#[test]
fn an_outline_lists_only_the_files_it_has_something_to_say_about() {
    let g = GitRepo::new();
    let body_v1 = b"pub fn add(a: u8, b: u8) -> u8 {\n    a + b\n}\n";
    let sig_v1 = b"pub fn tip(sha: Sha) -> Sha {\n    sha\n}\n";
    let base = g.commit_full(
        &[g.root],
        "base files\n",
        &[
            ("body.rs", body_v1),
            ("sig.rs", sig_v1),
            ("moved.txt", b"moved\n"),
            ("data.bin", b"\x00\x01\x02binary-one\n"),
        ],
        &[],
    );
    g.branch("main", base);

    let c1 = g.commit_full(
        &[base],
        &msg("feat: outline", "Ioutline1"),
        &[
            (
                "body.rs",
                b"pub fn add(a: u8, b: u8) -> u8 {\n    let sum = a + b;\n    sum\n}\n",
            ),
            ("sig.rs", b"pub fn tip(sha: &Sha) -> Sha {\n    sha\n}\n"),
            ("renamed.txt", b"moved\n"),
            ("data.bin", b"\x00\x01\x02binary-two\n"),
        ],
        &["moved.txt"],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_number(&pushed);

    let (st, diff) =
        http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff?mode=outline")));
    assert_eq!(st, 200, "{diff}");
    let paths: Vec<&str> = diff["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|f| f["path"].as_str().expect("a path"))
        .collect();
    assert_eq!(paths, ["/COMMIT_MSG", "sig.rs"], "{diff}");
}

#[test]
fn missing_revision_is_404() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("only one", "Ionly001"), &[("f.txt", "x\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_number(&pushed);

    let (st, _) = http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff")));
    assert_eq!(st, 200);
    let (st, e) = http_get(&server.url(&format!("/api/changes/{id}/revisions/1/diff")));
    assert_eq!(st, 404, "{e}");
    assert!(e["error"].as_str().unwrap().contains("revision 1"));

    let (st, e) = http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff?against=9")));
    assert_eq!(st, 404, "{e}");

    let missing = id + 999;
    let (st, _) = http_get(&server.url(&format!("/api/changes/{missing}/revisions/0/diff")));
    assert_eq!(st, 404);
}

/// `/lines` serves a renamed file's full text: the request names both sides,
/// which is what lets the tree diff be bounded to it and still pair the
/// rename.
#[test]
fn lines_of_a_renamed_file_need_both_of_its_names() {
    let g = GitRepo::new();
    let body = lines("l", 1..=40);
    let base = g.commit_full(&[g.root], "base\n", &[("old.txt", body.as_bytes())], &[]);
    g.branch("main", base);

    let moved = body.replace("l40\n", "l40 edited\n");
    let c1 = g.commit_full(
        &[base],
        &msg("rename it", "Irename01"),
        &[("new.txt", moved.as_bytes())],
        &["old.txt"],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_number(&pushed);

    let renamed = by_path(
        &http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff"))).1,
        "new.txt",
    );
    assert_eq!(renamed["status"], "renamed");
    assert_eq!(renamed["old_path"], "old.txt");

    let both = server.url(&format!(
        "/api/changes/{id}/revisions/0/lines?path=new.txt&old_path=old.txt"
    ));
    let (st, full) = http_get(&both);
    assert_eq!(st, 200, "{full}");
    let count = |v: &Value, kind: &str| {
        v["lines"]
            .as_array()
            .expect("lines array")
            .iter()
            .filter(|l| l["kind"] == kind)
            .count()
    };
    // The file's 39 untouched lines, plus the edited one on both sides.
    assert_eq!(count(&full, "context"), 39);
    assert_eq!((count(&full, "add"), count(&full, "del")), (1, 1));

    // Without the old name the bound holds one end of the rename, so nothing
    // pairs and the file reads as freshly added — which is what obliges a
    // client to send back the `old_path` it was given.
    let (st, one_sided) =
        http_get(&server.url(&format!("/api/changes/{id}/revisions/0/lines?path=new.txt")));
    assert_eq!(st, 200, "{one_sided}");
    assert_eq!(count(&one_sided, "context"), 0);
    assert_eq!(count(&one_sided, "add"), 40);
}
