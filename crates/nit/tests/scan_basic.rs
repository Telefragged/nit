//! Push basics over real HTTP (docs/api.md "Push"): an N-commit branch
//! becomes N changes each at revision 0, the derived chain lists exactly one
//! tip with its path ordered base→tip, a no-op re-push is idempotent,
//! extending the branch adds a change, an amend opens revision 1, and every
//! structural fault rejects the whole push with a 400.

mod common;

use common::{GitRepo, TestServer, http_get, member_id, msg, push};

fn only_chain(server: &TestServer) -> serde_json::Value {
    let (st, list) = http_get(&server.url("/api/chains"));
    assert_eq!(st, 200, "{list}");
    let chains = list["chains"].as_array().unwrap();
    assert_eq!(chains.len(), 1, "exactly one tip: {list}");
    chains[0].clone()
}

#[test]
fn push_creates_a_change_per_commit_at_revision_zero() {
    let g = GitRepo::new();
    let c1 = g.commit(
        &[g.root],
        &msg("server: add health", "I001"),
        &[("a.rs", "a\n")],
    );
    let c2 = g.commit(
        &[c1],
        &msg("server: add chains api", "I002"),
        &[("b.rs", "b\n")],
    );
    g.branch("feat", c2);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");

    let tip = &res["tip_change"];
    assert_eq!(tip["change_key"], "I002");
    assert_eq!(tip["revision"], 0);
    assert_eq!(tip["status"], "pending");

    let chain = only_chain(&server);
    let path = chain["path"].as_array().unwrap();
    assert_eq!(path.len(), 2);
    assert_eq!(path[0]["change_key"], "I001");
    assert_eq!(path[0]["position"], 0);
    assert_eq!(path[0]["revision"], 0);
    assert_eq!(path[0]["commit_sha"], c1.to_string());
    assert_eq!(
        path[0]["parent_sha"].as_str(),
        None,
        "PathEntry has no parent_sha"
    );
    assert_eq!(path[1]["change_key"], "I002");
    assert_eq!(path[1]["position"], 1);
    assert_eq!(path[1]["revision"], 0);
    assert_eq!(path[1]["commit_sha"], c2.to_string());

    let id1 = member_id(&server, &res, "I001");
    let (st, detail) = http_get(&server.url(&format!("/api/changes/{id1}")));
    assert_eq!(st, 200, "{detail}");
    let revs = detail["revisions"].as_array().unwrap();
    assert_eq!(revs.len(), 1);
    assert_eq!(revs[0]["number"], 0);
    assert_eq!(revs[0]["commit_sha"], c1.to_string());
    assert_eq!(revs[0]["parent_sha"], g.root.to_string());
}

#[test]
fn chains_lists_one_ordered_tip() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.rs", "b\n")]);
    let c3 = g.commit(&[c2], &msg("three", "I003"), &[("c.rs", "c\n")]);
    g.branch("feat", c3);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, _) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200);

    let chain = only_chain(&server);
    assert_eq!(chain["state"], "waiting_for_review");
    let path = chain["path"].as_array().unwrap();
    let keys: Vec<&str> = path
        .iter()
        .map(|m| m["change_key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["I001", "I002", "I003"]);
    for (i, m) in path.iter().enumerate() {
        assert_eq!(m["position"], i as u64, "0-based position");
        assert_eq!(m["revision"], 0);
        assert_eq!(m["status"], "pending");
    }
    assert_eq!(path[2]["commit_sha"], c3.to_string());
}

#[test]
fn no_op_repush_is_idempotent() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, _) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200);
    let id = member_id(&server, &only_chain(&server), "I001");

    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["tip_change"]["revision"], 0);
    let (_, detail) = http_get(&server.url(&format!("/api/changes/{id}")));
    assert_eq!(
        detail["revisions"].as_array().unwrap().len(),
        1,
        "still one revision"
    );
}

#[test]
fn extending_the_branch_adds_a_change() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, _) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200);

    let c2 = g.commit(&[c1], &msg("two", "I002"), &[("b.rs", "b\n")]);
    g.branch("feat", c2);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["tip_change"]["change_key"], "I002");
    assert_eq!(res["tip_change"]["revision"], 0);

    let path = only_chain(&server)["path"].as_array().unwrap().clone();
    assert_eq!(path.len(), 2);
    assert_eq!(path[1]["change_key"], "I002");
    assert_eq!(path[1]["position"], 1);

    let id1 = member_id(&server, &res, "I001");
    let (_, detail) = http_get(&server.url(&format!("/api/changes/{id1}")));
    assert_eq!(detail["revisions"].as_array().unwrap().len(), 1);
}

#[test]
fn amend_opens_revision_one_on_the_change() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200);
    let id = member_id(&server, &res, "I001");

    let c1b = g.commit(&[g.root], &msg("one", "I001"), &[("a.rs", "different\n")]);
    g.branch("feat", c1b);
    let (st, res) = push(&server, &g, "feat", "main");
    assert_eq!(st, 200, "{res}");
    assert_eq!(
        res["tip_change"]["change_id"], id,
        "same change across the amend"
    );
    assert_eq!(res["tip_change"]["revision"], 1);

    let (_, detail) = http_get(&server.url(&format!("/api/changes/{id}")));
    let revs = detail["revisions"].as_array().unwrap();
    assert_eq!(revs.len(), 2);
    assert_eq!(revs[1]["number"], 1);
    assert_eq!(revs[1]["commit_sha"], c1b.to_string());
}

#[test]
fn merge_commit_rejects_the_push() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    let side = g.commit(&[g.root], &msg("side", "I00s"), &[("s.rs", "s\n")]);
    let merge = g.commit(&[c1, side], "Merge side into feat\n", &[]);
    g.branch("feat", merge);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 400, "{e}");
    assert!(
        e["error"].as_str().unwrap().contains("merge commits"),
        "{e}"
    );
    // All-or-nothing: c1 alone would push fine, but the merge commit voids it too.
    let (_, list) = http_get(&server.url("/api/chains"));
    assert!(list["chains"].as_array().unwrap().is_empty(), "{list}");
}

#[test]
fn already_merged_commit_rejects_the_push() {
    // A tip that is ancestor-or-equal of the base walks to nothing — the work
    // already landed. The push fails (409) rather than silently recording
    // nothing (docs/data-model.md "Push").
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("main", c1);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 409, "{e}");
    assert!(
        e["error"].as_str().unwrap().contains("already merged"),
        "{e}"
    );
    // All-or-nothing: the already-merged push records no changes either.
    let (_, list) = http_get(&server.url("/api/chains"));
    assert!(list["chains"].as_array().unwrap().is_empty(), "{list}");
}

#[test]
fn missing_change_id_rejects_the_push() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], "no trailer here\n", &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 400, "{e}");
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("without a Change-Id trailer"),
        "{e}"
    );
}

#[test]
fn duplicate_change_id_rejects_the_push() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "Idup"), &[("a.rs", "a\n")]);
    let c2 = g.commit(&[c1], &msg("two", "Idup"), &[("b.rs", "b\n")]);
    g.branch("feat", c2);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 400, "{e}");
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("duplicate Change-Id Idup"),
        "{e}"
    );
    let (_, list) = http_get(&server.url("/api/chains"));
    assert!(
        list["chains"].as_array().unwrap().is_empty(),
        "nothing recorded: {list}"
    );
}

#[test]
fn fixup_subject_rejects_the_push() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.rs", "a\n")]);
    let fx = g.commit(&[c1], &msg("fixup! one", "I00f"), &[("a.rs", "a2\n")]);
    g.branch("feat", fx);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, e) = push(&server, &g, "feat", "main");
    assert_eq!(st, 400, "{e}");
    assert!(
        e["error"].as_str().unwrap().contains("fixup!/squash!"),
        "{e}"
    );
}
