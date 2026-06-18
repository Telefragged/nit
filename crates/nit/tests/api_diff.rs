//! The diff endpoint (docs/api.md "Diff"): `/COMMIT_MSG` leads every
//! response, real files carry exact add/del counts and hunk lines, binary
//! files have empty hunks, and `?against` produces an interdiff whose
//! `/COMMIT_MSG` is a real message diff. Revisions (0-based) are minted by
//! `push` — amend + re-push gives revision 1.

mod common;

use common::{GitRepo, TestServer, http_get, msg, push};
use serde_json::{Value, json};

/// `prefix1\nprefix2\n…\nprefixN\n` — numbered lines, newline-terminated.
fn lines(prefix: &str, n: std::ops::RangeInclusive<i64>) -> String {
    use std::fmt::Write;
    n.fold(String::new(), |mut s, i| {
        writeln!(s, "{prefix}{i}").unwrap();
        s
    })
}

/// The pushed tip change's id (revision 0 lives there after the first push).
fn tip_change_id(push_result: &Value) -> u64 {
    push_result["tip_change"]["change_id"]
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

    // The change touches two far-apart lines (two hunks), adds a file, and
    // rewrites the binary blob.
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
    let (st, pushed) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_id(&pushed);
    assert_eq!(pushed["tip_change"]["revision"], 0, "first revision is 0");

    let (st, diff) = http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff")));
    assert_eq!(st, 200, "{diff}");

    // The synthetic commit-message file is the FIRST entry, the whole message
    // as one all-add hunk (docs/api.md "The commit message as a file").
    assert_eq!(
        diff["files"][0],
        json!({
            "path": "/COMMIT_MSG",
            "status": "added",
            "binary": false,
            "additions": 3,
            "deletions": 0,
            "hunks": [{
                "old_start": 0, "old_lines": 0, "new_start": 1, "new_lines": 3,
                "header": "",
                "lines": [
                    {"kind": "add", "new": 1, "text": "feat: diff"},
                    {"kind": "add", "new": 2, "text": ""},
                    {"kind": "add", "new": 3, "text": "Change-Id: Idiff0001"},
                ],
            }],
        })
    );

    // /COMMIT_MSG + the three real files.
    assert_eq!(diff["files"].as_array().unwrap().len(), 4);

    // A multi-hunk modification with exact line numbers and texts.
    assert_eq!(
        by_path(&diff, "keep.txt"),
        json!({
            "path": "keep.txt",
            "status": "modified",
            "binary": false,
            "additions": 2,
            "deletions": 2,
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

    // An added file: one all-add hunk, 1-based new line numbers.
    assert_eq!(
        by_path(&diff, "fresh.txt"),
        json!({
            "path": "fresh.txt",
            "status": "added",
            "binary": false,
            "additions": 2,
            "deletions": 0,
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

    // A binary modification: flagged, no hunks, counts zero.
    assert_eq!(
        by_path(&diff, "data.bin"),
        json!({
            "path": "data.bin",
            "status": "modified",
            "binary": true,
            "additions": 0,
            "deletions": 0,
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

    // Revision 0: the original commit.
    let c1 = g.commit(
        &[base],
        &msg("feat: thing", "Iinter01"),
        &[("body.txt", &lines("b", 1..=8).replace("b4\n", "b4 v1\n"))],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_id(&pushed);
    assert_eq!(pushed["tip_change"]["revision"], 0);

    // Revision 1: amend — reword the subject and re-edit the same line.
    let c2 = g.commit(
        &[base],
        &msg("feat: thing, reworded", "Iinter01"),
        &[("body.txt", &lines("b", 1..=8).replace("b4\n", "b4 v2\n"))],
    );
    g.branch("feat", c2);
    let (st, pushed) = push(&server, &g, "feat", "main", None);
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

    // tree(r0) → tree(r1): only body.txt's line 4 moved (b4 v1 → b4 v2).
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
#[test]
fn missing_revision_is_404() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("only one", "Ionly001"), &[("f.txt", "x\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, pushed) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200, "{pushed}");
    let id = tip_change_id(&pushed);

    // Revision 0 exists; revision 1 does not.
    let (st, _) = http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff")));
    assert_eq!(st, 200);
    let (st, e) = http_get(&server.url(&format!("/api/changes/{id}/revisions/1/diff")));
    assert_eq!(st, 404, "{e}");
    assert!(e["error"].as_str().unwrap().contains("revision 1"));

    // A missing `against` revision is equally a 404.
    let (st, e) = http_get(&server.url(&format!("/api/changes/{id}/revisions/0/diff?against=9")));
    assert_eq!(st, 404, "{e}");

    // An unknown change is a 404 too.
    let missing = id + 999;
    let (st, _) = http_get(&server.url(&format!("/api/changes/{missing}/revisions/0/diff")));
    assert_eq!(st, 404);
}
