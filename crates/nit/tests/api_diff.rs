//! Golden test for the Diff JSON shape (docs/api.md): a single revision
//! diff containing a multi-hunk modification, a rename with a tweak, a
//! binary change and an added file — exact wire output.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, msg};
use serde_json::{Value, json};

fn lines(prefix: &str, n: std::ops::RangeInclusive<i64>) -> String {
    use std::fmt::Write;
    n.fold(String::new(), |mut s, i| {
        writeln!(s, "{prefix}{i}").unwrap();
        s
    })
}

#[test]
#[expect(clippy::too_many_lines, reason = "linear end-to-end scenario")]
fn diff_json_golden() {
    let g = GitRepo::new();
    let keep_v1 = lines("k", 1..=12);
    let renamed_v1 = lines("r", 1..=40);
    let base = g.commit_full(
        &[g.root],
        "base files\n",
        &[
            ("keep.txt", keep_v1.as_bytes()),
            ("old_name.txt", renamed_v1.as_bytes()),
            ("data.bin", b"\x00\x01\x02binary-one\n"),
        ],
        &[],
    );
    g.branch("main", base);

    let keep_v2 = keep_v1
        .replace("k2\n", "k2 changed\n")
        .replace("k11\n", "k11 changed\n");
    let renamed_v2 = renamed_v1.replace("r40\n", "r40 changed\n");
    let c1 = g.commit_full(
        &[base],
        &msg("feat: golden", "Igold"),
        &[
            ("keep.txt", keep_v2.as_bytes()),
            ("new_name.txt", renamed_v2.as_bytes()),
            ("data.bin", b"\x00\x01\x02binary-two\n"),
            ("fresh.txt", b"hello\nworld\n"),
        ],
        &["old_name.txt"],
    );
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, chain) = http_post(
        &server.url("/api/chains"),
        &json!({
            "repo_path": g.workdir().to_string_lossy(),
            "branch": "feat",
            "base": "main",
        }),
    );
    assert_eq!(st, 200, "{chain}");
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    let (st, diff) = http_get(&server.url(&format!("/api/changes/{change_id}/revisions/1/diff")));
    assert_eq!(st, 200, "{diff}");

    // The synthetic commit-message file leads the response (docs/api.md
    // "The commit message as a file") — asserted before sorting.
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
                    {"kind": "add", "new": 1, "text": "feat: golden"},
                    {"kind": "add", "new": 2, "text": ""},
                    {"kind": "add", "new": 3, "text": "Change-Id: Igold"},
                ],
            }],
        })
    );

    let mut files = diff["files"].as_array().unwrap().clone();
    files.sort_by_key(|f| f["path"].as_str().unwrap().to_string());
    let by_path = |files: &[Value], p: &str| {
        files
            .iter()
            .find(|f| f["path"] == p)
            .unwrap_or_else(|| panic!("no file {p} in {files:?}"))
            .clone()
    };

    assert_eq!(files.len(), 5);

    assert_eq!(
        by_path(&files, "data.bin"),
        json!({
            "path": "data.bin",
            "status": "modified",
            "binary": true,
            "additions": 0,
            "deletions": 0,
            "hunks": [],
        })
    );

    assert_eq!(
        by_path(&files, "fresh.txt"),
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

    assert_eq!(
        by_path(&files, "keep.txt"),
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

    assert_eq!(
        by_path(&files, "new_name.txt"),
        json!({
            "path": "new_name.txt",
            "old_path": "old_name.txt",
            "status": "renamed",
            "binary": false,
            "additions": 1,
            "deletions": 1,
            "hunks": [{
                "old_start": 37, "old_lines": 4, "new_start": 37, "new_lines": 4,
                "header": "r36",
                "lines": [
                    {"kind": "context", "old": 37, "new": 37, "text": "r37"},
                    {"kind": "context", "old": 38, "new": 38, "text": "r38"},
                    {"kind": "context", "old": 39, "new": 39, "text": "r39"},
                    {"kind": "del", "old": 40, "text": "r40"},
                    {"kind": "add", "new": 40, "text": "r40 changed"},
                ],
            }],
        })
    );
}
