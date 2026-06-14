//! The full review loop over real HTTP against a real repo:
//! push → drafts → review (streamed on /events) → reply → amend push (new
//! revisions + comment porting) → interdiff → stale-review 409 →
//! approvals → merge detection. docs/api.md end to end.

mod common;

use common::{GitRepo, TestServer, http_delete, http_get, http_patch, http_post, msg, sse_collect};
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
fn full_review_loop() {
    let g = GitRepo::new();
    let lib_v1 = lines("L", 1..=12);
    let c1 = g.commit(
        &[g.root],
        &msg("server: add api", "Ione"),
        &[("src/lib.rs", &lib_v1)],
    );
    let c2 = g.commit(
        &[c1],
        &msg("docs: add docs", "Itwo"),
        &[("docs.md", "docs\n")],
    );
    g.branch("feat", c2);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let register = json!({
        "repo_path": g.workdir().to_string_lossy(),
        "branch": "feat",
        "base": "main",
    });

    // --- push (register + forced scan) -----------------------------------
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    let chain_id = chain["id"].as_i64().unwrap();
    assert_eq!(chain["status"], "active");
    assert_eq!(chain["state"], "waiting_for_review");
    assert_eq!(chain["last_scan_error"], Value::Null);
    assert_eq!(
        chain["web_url"],
        json!(format!("{}/chains/{chain_id}", server.base))
    );
    let changes = chain["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["subject"], "server: add api");
    assert_eq!(changes[0]["change_key"], "Ione");
    assert_eq!(changes[0]["status"], "pending");
    assert_eq!(changes[0]["revision"], 1);
    assert_eq!(changes[0]["last_reviewed_revision"], Value::Null);
    assert_eq!(changes[0]["commit_sha"], c1.to_string());
    assert_eq!(changes[1]["position"], 1);
    let change1 = changes[0]["id"].as_i64().unwrap();
    let change2 = changes[1]["id"].as_i64().unwrap();

    // Idempotent re-push: same chain, same changes.
    let (st, again) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200);
    assert_eq!(again["id"].as_i64().unwrap(), chain_id);
    assert_eq!(again["changes"].as_array().unwrap().len(), 2);

    // Error shapes: unknown chain 404, unresolvable registration 400.
    let (st, e) = http_get(&server.url("/api/chains/999"));
    assert_eq!(st, 404);
    assert!(e["error"].as_str().unwrap().contains("999"));
    let (st, e) = http_post(
        &server.url("/api/chains"),
        &json!({"repo_path": "/does/not/exist", "branch": "b", "base": "main"}),
    );
    assert_eq!(st, 400, "{e}");
    let (st, e) = http_post(
        &server.url("/api/chains"),
        &json!({"repo_path": g.workdir().to_string_lossy(), "branch": "nope", "base": "main"}),
    );
    assert_eq!(st, 400, "{e}");

    // Dashboard list.
    let (st, list) = http_get(&server.url("/api/chains"));
    assert_eq!(st, 200);
    assert_eq!(list["chains"].as_array().unwrap().len(), 1);

    // --- reviewer drafts ---------------------------------------------------
    let drafts_url = server.url(&format!("/api/changes/{change1}/drafts"));
    let (st, draft_a) = http_post(
        &drafts_url,
        &json!({"revision": 1, "file": "src/lib.rs", "line": 3, "body": "rename this"}),
    );
    assert_eq!(st, 200, "{draft_a}");
    assert_eq!(draft_a["state"], "draft");
    assert_eq!(draft_a["author"], "reviewer");
    assert_eq!(draft_a["side"], "new");
    assert_eq!(draft_a["line_text"], "L3");
    assert_eq!(draft_a["revision"], 1);
    assert_eq!(draft_a["line"], 3);
    assert!(
        draft_a.get("rendered_line").is_none(),
        "comments carry no ported fields"
    );
    let draft_a_id = draft_a["id"].as_i64().unwrap();

    let (st, draft_b) = http_post(
        &drafts_url,
        &json!({"revision": 1, "file": "src/lib.rs", "line": 10, "body": "typo"}),
    );
    assert_eq!(st, 200);
    let draft_b_id = draft_b["id"].as_i64().unwrap();

    // Change-level draft: edit it, then delete it.
    let (st, draft_c) = http_post(&drafts_url, &json!({"revision": 1, "body": "overall"}));
    assert_eq!(st, 200);
    assert_eq!(draft_c["file"], Value::Null);
    let draft_c_id = draft_c["id"].as_i64().unwrap();
    let (st, edited) = http_patch(
        &server.url(&format!("/api/drafts/{draft_c_id}")),
        &json!({"body": "overall, but stronger"}),
    );
    assert_eq!(st, 200);
    assert_eq!(edited["body"], "overall, but stronger");
    let (st, _) = http_delete(&server.url(&format!("/api/drafts/{draft_c_id}")));
    assert_eq!(st, 204);
    let (st, _) = http_delete(&server.url(&format!("/api/drafts/{draft_c_id}")));
    assert_eq!(st, 404);

    let (_, chain_now) = http_get(&server.url(&format!("/api/chains/{chain_id}")));
    assert_eq!(chain_now["changes"][0]["counts"]["drafts"], 2);

    // --- the review streams on /events, the fold flips to agents_turn ------
    let (_, boot) = http_get(&server.url(&format!("/api/chains/{chain_id}/feedback")));
    assert_eq!(boot["state"], "waiting_for_review");
    assert_eq!(boot["actionable"], false);
    let (_, log) = http_get(&server.url(&format!("/api/chains/{chain_id}/log")));
    let cursor = log["head"].as_i64().unwrap();
    assert!(cursor > 0, "push scan must have appended a revisions entry");

    // Park an SSE reader caught up at head, then submit a review; the review
    // entry must stream to it (the server emits every entry — relevance is
    // the client's call).
    let events_url = server.url(&format!("/api/chains/{chain_id}/events?cursor={cursor}"));
    let reader =
        std::thread::spawn(move || sse_collect(&events_url, 1, std::time::Duration::from_secs(5)));
    std::thread::sleep(std::time::Duration::from_millis(300));

    let (st, submitted) = http_post(
        &server.url(&format!("/api/changes/{change1}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "please fix"}),
    );
    assert_eq!(st, 200, "{submitted}");
    assert_eq!(submitted["review"]["verdict"], "request_changes");
    assert_eq!(submitted["review"]["revision"], 1);
    let published = submitted["published_comments"].as_array().unwrap();
    assert_eq!(published.len(), 2);
    assert!(published.iter().all(|c| c["state"] == "published"));
    let review_id = submitted["review"]["id"].as_i64().unwrap();
    assert!(
        published
            .iter()
            .all(|c| c["review_id"].as_i64() == Some(review_id))
    );

    let streamed = reader.join().unwrap();
    assert_eq!(streamed.len(), 1, "{streamed:?}");
    assert_eq!(streamed[0]["kind"], "review");
    assert_eq!(streamed[0]["idx"].as_i64(), Some(cursor));

    let (_, feedback) = http_get(&server.url(&format!("/api/chains/{chain_id}/feedback")));
    assert_eq!(feedback["state"], "agents_turn");
    assert_eq!(feedback["actionable"], true);
    let fb_change = &feedback["changes"][0];
    assert_eq!(fb_change["status"], "changes_requested");
    assert_eq!(fb_change["review"]["verdict"], "request_changes");
    assert_eq!(fb_change["review"]["message"], "please fix");
    assert_eq!(fb_change["unresolved"], 2);
    assert_eq!(fb_change["comments"].as_array().unwrap().len(), 2);

    // Chain state flips for the dashboard too.
    let (_, chain_now) = http_get(&server.url(&format!("/api/chains/{chain_id}")));
    assert_eq!(chain_now["state"], "agents_turn");
    assert_eq!(chain_now["changes"][0]["status"], "changes_requested");
    assert_eq!(chain_now["changes"][0]["last_reviewed_revision"], 1);

    // --- agent replies and resolves one thread -----------------------------
    let (st, reply) = http_post(
        &server.url(&format!("/api/comments/{draft_a_id}/replies")),
        &json!({"body": "renamed in r2", "resolved": true}),
    );
    assert_eq!(st, 200, "{reply}");
    assert_eq!(reply["author"], "agent");
    assert_eq!(reply["state"], "published");
    assert_eq!(reply["parent_id"].as_i64(), Some(draft_a_id));

    let (_, feedback) = http_get(&server.url(&format!("/api/chains/{chain_id}/feedback")));
    assert_eq!(feedback["changes"][0]["unresolved"], 1);
    // Latest review's threads (A incl. reply, B) stay in feedback scope.
    assert_eq!(
        feedback["changes"][0]["comments"].as_array().unwrap().len(),
        3
    );

    // --- amend push: new revisions, comments stay pinned --------------------
    // The agent amends change one in place (same Change-Id) and rebases
    // change two on top. L0 inserted on top, L3 itself rewritten — none of
    // which moves the published comments: each stays pinned to revision 1,
    // shown only in a diff whose range displays r1 (docs/api.md placement).
    let lib_v2 = format!("L0\n{}", lib_v1.replace("L3\n", "L3 changed\n"));
    let c1b = g.commit(
        &[g.root],
        &msg("server: add api", "Ione"),
        &[("src/lib.rs", &lib_v2)],
    );
    let c2b = g.commit(
        &[c1b],
        &msg("docs: add docs", "Itwo"),
        &[("docs.md", "docs\n")],
    );
    g.branch("feat", c2b);

    let (st, chain_now) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200);
    let ch1 = &chain_now["changes"][0];
    assert_eq!(ch1["revision"], 2);
    assert_eq!(
        ch1["status"], "pending",
        "content changed → reviewer looks again"
    );
    assert_eq!(ch1["counts"]["revisions"], 2);
    assert_eq!(ch1["last_reviewed_revision"], 1);

    let (st, detail) = http_get(&server.url(&format!("/api/changes/{change1}")));
    assert_eq!(st, 200);
    assert_eq!(detail["revisions"].as_array().unwrap().len(), 2);
    let rev2 = &detail["revisions"][1];
    assert_eq!(rev2["number"], 2);
    assert_eq!(rev2["commit_sha"], c1b.to_string());
    let comments = detail["comments"].as_array().unwrap();
    let by_id = |id: i64| {
        comments
            .iter()
            .find(|c| c["id"].as_i64() == Some(id))
            .unwrap()
    };
    // Both published comments keep the revision-1 anchor they were written
    // on — the server ports nothing onto revision 2, and the wire carries
    // no rendered_line/outdated for the client to misread.
    let a = by_id(draft_a_id);
    assert_eq!(a["revision"], 1);
    assert_eq!(a["line"], 3);
    assert_eq!(a["side"], "new");
    assert_eq!(a["line_text"], "L3", "snapshot shows what was commented on");
    assert!(a.get("rendered_line").is_none());
    assert!(a.get("outdated").is_none());
    let b = by_id(draft_b_id);
    assert_eq!(b["revision"], 1);
    assert_eq!(b["line"], 10);

    // --- diff + interdiff ---------------------------------------------------
    // /COMMIT_MSG leads every diff response: all-add vs parent.
    let (st, diff) = http_get(&server.url(&format!("/api/changes/{change1}/revisions/2/diff")));
    assert_eq!(st, 200);
    let files = diff["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["path"], "/COMMIT_MSG");
    assert_eq!(files[0]["status"], "added");
    assert_eq!(files[0]["additions"], 3); // subject, blank, Change-Id
    assert_eq!(files[1]["path"], "src/lib.rs");
    assert_eq!(files[1]["status"], "added");
    assert_eq!(files[1]["additions"], 13);

    let (st, interdiff) = http_get(&server.url(&format!(
        "/api/changes/{change1}/revisions/2/diff?against=1"
    )));
    assert_eq!(st, 200);
    let files = interdiff["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    // The message is identical between r1 and r2 (the amend doesn't touch
    // it): one all-context hunk, zero counts.
    assert_eq!(files[0]["path"], "/COMMIT_MSG");
    assert_eq!(files[0]["status"], "modified");
    assert_eq!(files[0]["additions"], 0);
    assert_eq!(files[0]["deletions"], 0);
    let msg_lines = files[0]["hunks"][0]["lines"].as_array().unwrap();
    assert!(msg_lines.iter().all(|l| l["kind"] == "context"));
    assert_eq!(files[1]["path"], "src/lib.rs");
    assert_eq!(files[1]["status"], "modified");
    assert_eq!(files[1]["additions"], 2);
    assert_eq!(files[1]["deletions"], 1);

    let (st, _) = http_get(&server.url(&format!("/api/changes/{change1}/revisions/9/diff")));
    assert_eq!(st, 404);

    // --- stale review → 409 -------------------------------------------------
    let (st, e) = http_post(
        &server.url(&format!("/api/changes/{change1}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": ""}),
    );
    assert_eq!(st, 409, "{e}");
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("no longer the latest")
    );

    // --- approve everything → approved --------------------------------
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change1}/reviews")),
        &json!({"revision": 2, "verdict": "approve", "message": "lgtm"}),
    );
    assert_eq!(st, 200);
    // Change two's rebase was pure (same diff, same message): a review
    // against revision 1 auto-retargets to revision 2.
    let (st, retargeted) = http_post(
        &server.url(&format!("/api/changes/{change2}/reviews")),
        &json!({"revision": 1, "verdict": "approve", "message": ""}),
    );
    assert_eq!(st, 200, "{retargeted}");
    assert_eq!(retargeted["review"]["revision"], 2);

    let (_, feedback) = http_get(&server.url(&format!("/api/chains/{chain_id}/feedback")));
    assert_eq!(feedback["state"], "approved");
    assert_eq!(feedback["actionable"], true);

    // --- ff-merge → chain leaves the dashboard -------------------------------
    // The chain is already in its final shape: fast-forward main.
    g.branch("main", c2b);

    let (st, merged) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200);
    assert_eq!(merged["status"], "merged", "{merged}");
    assert_eq!(merged["state"], "merged");

    let (_, list) = http_get(&server.url("/api/chains"));
    assert_eq!(list["chains"].as_array().unwrap().len(), 0);
    let (_, list) = http_get(&server.url("/api/chains?status=all"));
    assert_eq!(list["chains"].as_array().unwrap().len(), 1);
    assert_eq!(list["chains"][0]["status"], "merged");

    let (_, feedback) = http_get(&server.url(&format!("/api/chains/{chain_id}/feedback")));
    assert_eq!(feedback["state"], "merged");
    assert_eq!(feedback["actionable"], true);
}

#[test]
fn partial_flag_is_sticky_and_flips_append_entries() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: a", "Ia"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let mut register = json!({
        "repo_path": g.workdir().to_string_lossy(),
        "branch": "feat",
        "base": "main",
    });

    // Fresh registration without partial: defaults to not-partial.
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    assert_eq!(chain["partial"], false);
    let chain_id = chain["id"].as_i64().unwrap();
    let cursor_of = || {
        let (st, log) = http_get(&server.url(&format!("/api/chains/{chain_id}/log")));
        assert_eq!(st, 200, "{log}");
        let (_, feedback) = http_get(&server.url(&format!("/api/chains/{chain_id}/feedback")));
        (log["head"].as_i64().unwrap(), feedback)
    };
    let (registered, _) = cursor_of();

    // partial: true flips the flag and appends a `partial` entry even though
    // nothing structural changed.
    register["partial"] = json!(true);
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    assert_eq!(chain["partial"], true);
    let (marked, feedback) = cursor_of();
    assert!(marked > registered, "a partial flip must emit an event");
    assert_eq!(feedback["chain"]["partial"], true);

    // Absent partial leaves the flag alone (sticky) and, with no structural
    // difference either, emits nothing.
    register.as_object_mut().unwrap().remove("partial");
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    assert_eq!(chain["partial"], true);
    let (repushed, _) = cursor_of();
    assert_eq!(repushed, marked);

    // Re-sending the stored value is not a flip.
    register["partial"] = json!(true);
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    assert_eq!(chain["partial"], true);
    let (resent, _) = cursor_of();
    assert_eq!(resent, marked);

    // partial: false clears the flag and emits again (nit ready).
    register["partial"] = json!(false);
    let (st, chain) = http_post(&server.url("/api/chains"), &register);
    assert_eq!(st, 200, "{chain}");
    assert_eq!(chain["partial"], false);
    let (cleared, feedback) = cursor_of();
    assert!(cleared > marked, "clearing partial must emit an event");
    assert_eq!(feedback["chain"]["partial"], false);
}
