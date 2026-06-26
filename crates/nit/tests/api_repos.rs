//! The repo registry over HTTP (docs/api.md "Repos"). A repo is registered
//! explicitly with `nit repo create` (`POST /api/repos`), pinning its one
//! canonical `base_branch`; its identity is the git-common-dir, and a push
//! into an unregistered repo is a 404. `GET /api/repos` lists each repo with
//! its live-tip `active_chains` count, which excludes a fully merged/abandoned
//! chain (decided only by the background timer). `GET /api/chains?repo={id}`
//! scopes the chain list to one repo, and `PATCH /api/repos/{id}`
//! (≡ `nit repo move`) repoints a repo after a disk move (404 unknown, 400
//! unresolvable, 409 collision).

mod common;

use std::time::Duration;

use common::{
    GitRepo, TestServer, create_repo, fast_timer, http_get, http_patch, http_post, member_id, msg,
    push, wait_for,
};
use serde_json::json;

/// Canonical git-common-dir of a checkout rooted at `root` (its `.git` child),
/// as the string nit stores — for asserting a relocated path.
fn git_dir_of(root: &std::path::Path) -> String {
    std::fs::canonicalize(root.join(".git"))
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// The `active_chains` of repo `id` in a `GET /api/repos` body.
fn active_chains(server: &TestServer, id: u64) -> u64 {
    let (st, list) = http_get(&server.url("/api/repos"));
    assert_eq!(st, 200, "{list}");
    list["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_u64() == Some(id))
        .unwrap_or_else(|| panic!("repo {id} missing from {list}"))
        .get("active_chains")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("no active_chains on repo {id}"))
}

#[test]
fn repos_list_shape_base_branch_and_scoped_chains() {
    // Two distinct repos (distinct git dirs); the second carries two chains —
    // `feat` and `topic` both fork straight off `main`, so each is its own
    // live tip (a leaf in the parent DAG), not one stacked on the other.
    let a = GitRepo::new();
    let a1 = a.commit(&[a.root], &msg("a: one", "Ia1"), &[("a.rs", "a\n")]);
    a.branch("feat", a1);

    let b = GitRepo::new();
    let b1 = b.commit(&[b.root], &msg("b: one", "Ib1"), &[("b.rs", "b\n")]);
    b.branch("feat", b1);
    let b2 = b.commit(&[b.root], &msg("b: two", "Ib2"), &[("b2.rs", "b2\n")]);
    b.branch("topic", b2);

    let server = TestServer::start(a.dir.path().join("nit.sqlite3"), None);

    // The `push` helper registers each repo first (base `main`), then pushes.
    let (st, _) = push(&server, &a, "feat", "main", None);
    assert_eq!(st, 200);
    // Two independent tips off main → two live chains in repo b.
    let (st, _) = push(&server, &b, "feat", "main", None);
    assert_eq!(st, 200);
    let (st, _) = push(&server, &b, "topic", "main", None);
    assert_eq!(st, 200);

    let (st, list) = http_get(&server.url("/api/repos"));
    assert_eq!(st, 200);
    let repos = list["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 2, "{list}");

    let by_dir = |dir: &str| {
        repos
            .iter()
            .find(|r| r["git_dir"].as_str() == Some(dir))
            .unwrap_or_else(|| panic!("repo {dir} missing from {list}"))
    };
    let repo_a = by_dir(&a.git_dir());
    let repo_b = by_dir(&b.git_dir());
    let id_a = repo_a["id"].as_u64().unwrap();
    let id_b = repo_b["id"].as_u64().unwrap();
    assert_ne!(id_a, id_b, "distinct git dirs are distinct repos");

    // Repo shape: id, git_dir, base_branch (set at create), active_chains.
    assert_eq!(repo_a["base_branch"], "main");
    assert_eq!(repo_b["base_branch"], "main");
    assert_eq!(active_chains(&server, id_a), 1);
    assert_eq!(active_chains(&server, id_b), 2, "two independent tips");

    // GET /api/chains?repo=: scoped to one repo's tips only.
    let (st, scoped_b) = http_get(&server.url(&format!("/api/chains?repo={id_b}")));
    assert_eq!(st, 200);
    let b_chains = scoped_b["chains"].as_array().unwrap();
    assert_eq!(b_chains.len(), 2, "{scoped_b}");

    let (st, scoped_a) = http_get(&server.url(&format!("/api/chains?repo={id_a}")));
    assert_eq!(st, 200);
    assert_eq!(scoped_a["chains"].as_array().unwrap().len(), 1);

    // An unknown repo filter scopes to nothing (not an error).
    let (st, none) = http_get(&server.url("/api/chains?repo=9999"));
    assert_eq!(st, 200);
    assert!(none["chains"].as_array().unwrap().is_empty());
}

#[test]
fn base_can_be_any_branch() {
    // The base is whatever `--base` names — nit never assumes `main`/`master`.
    // A repo without either registers fine against an arbitrary `trunk`.
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("base: one", "Ib1"), &[("a.rs", "a\n")]);
    g.branch("trunk", c1);
    let c2 = g.commit(&[c1], &msg("core: two", "Ic2"), &[("b.rs", "b\n")]);
    g.branch("feat", c2);
    g.delete_branch("main");
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, res) = push(&server, &g, "feat", "trunk", None);
    assert_eq!(st, 200, "{res}");
    assert_eq!(repo_base(&server, first_repo(&server)), "trunk");
}

#[test]
fn create_repo_registers_and_pins_base() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: one", "Ic1"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    // A second canonical-branch candidate, to prove a re-create can't switch it.
    g.branch("trunk", g.root);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // An explicit base registers the repo and pins its canonical branch; no
    // push has happened, so it carries no live tips.
    let (st, repo) = create_repo(&server, &g, "main");
    assert_eq!(st, 200, "{repo}");
    assert_eq!(repo["git_dir"].as_str().unwrap(), g.git_dir());
    assert_eq!(repo["base_branch"], "main");
    assert_eq!(repo["active_chains"].as_u64(), Some(0));
    let id = first_repo(&server);

    // Re-registering is a 409 even when it names a different base — create means
    // create, and the pinned base is fixed (one canonical branch per repo).
    let (st, err) = create_repo(&server, &g, "trunk");
    assert_eq!(st, 409, "{err}");
    assert!(
        err["error"]
            .as_str()
            .unwrap()
            .contains("already registered"),
        "{err}"
    );
    assert_eq!(repo_base(&server, id), "main");
}

#[test]
fn push_into_unregistered_repo_is_404() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: one", "Ic1"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    // No `nit repo create` first: the push is rejected and nothing is recorded.
    let unregistered = |tip: &str| {
        http_post(
            &server.url("/api/push"),
            &json!({"git_dir": g.git_dir(), "tip": tip}),
        )
    };
    let (st, err) = unregistered("feat");
    assert_eq!(st, 404, "{err}");
    assert!(
        err["error"].as_str().unwrap().contains("not registered"),
        "{err}"
    );
    let (_, list) = http_get(&server.url("/api/repos"));
    assert!(
        list["repos"].as_array().unwrap().is_empty(),
        "a rejected push registers nothing: {list}"
    );

    // After create, the same push lands.
    let (st, _) = create_repo(&server, &g, "main");
    assert_eq!(st, 200);
    let (st, res) = unregistered("feat");
    assert_eq!(st, 200, "{res}");
}

#[test]
fn create_repo_rejects_unknown_base() {
    // Naming a branch the repo doesn't have is a 400 — nit never guesses a base.
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: one", "Ic1"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, err) = create_repo(&server, &g, "nope");
    assert_eq!(st, 400, "{err}");
    assert!(err["error"].as_str().unwrap().contains("nope"), "{err}");
}

#[test]
fn nit_repo_create_cli() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: one", "Ic1"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_nit"))
            .args(args)
            .current_dir(g.workdir())
            .env("NIT_SERVER", &server.base)
            .output()
            .expect("running nit repo create")
    };

    // `--base` is required: a bare create can't even parse.
    assert!(!run(&["repo", "create"]).status.success());

    // `nit repo create --base main` from inside the repo registers the git dir.
    let out = run(&["repo", "create", "--base", "main"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let repo: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(repo["git_dir"].as_str().unwrap(), g.git_dir());
    assert_eq!(repo["base_branch"], "main");

    // The repo now shows up in `nit repo list` (≡ GET /api/repos).
    let (_, list) = http_get(&server.url("/api/repos"));
    assert_eq!(list["repos"][0]["git_dir"].as_str().unwrap(), g.git_dir());
}

#[test]
fn relocate_repo_endpoint() {
    let a = GitRepo::new();
    let a1 = a.commit(&[a.root], &msg("a: one", "Ia1"), &[("a.rs", "a\n")]);
    a.branch("feat", a1);
    let server = TestServer::start(a.dir.path().join("nit.sqlite3"), None);
    let (st, _) = push(&server, &a, "feat", "main", None);
    assert_eq!(st, 200);
    let repo_id = first_repo(&server);

    // Unknown repo → 404.
    let (st, _) = http_patch(
        &server.url("/api/repos/999"),
        &json!({"git_dir": a.git_dir()}),
    );
    assert_eq!(st, 404);

    // An unresolvable new path → 400.
    let (st, _) = http_patch(
        &server.url(&format!("/api/repos/{repo_id}")),
        &json!({"git_dir": "/does/not/exist/.git"}),
    );
    assert_eq!(st, 400);

    // A path belonging to a *different* repo → 409. Register a second repo,
    // then try to point repo A at its git dir.
    let b = GitRepo::new();
    let b1 = b.commit(&[b.root], &msg("b: one", "Ib1"), &[("b.rs", "b\n")]);
    b.branch("feat", b1);
    let (st, _) = push(&server, &b, "feat", "main", None);
    assert_eq!(st, 200);
    let (st, conflict) = http_patch(
        &server.url(&format!("/api/repos/{repo_id}")),
        &json!({"git_dir": b.git_dir()}),
    );
    assert_eq!(st, 409, "{conflict}");

    // Move repo A on disk, then repoint it at the new git dir.
    let new_root = a.dir.path().join("moved");
    std::fs::rename(a.dir.path().join("repo"), &new_root).unwrap();
    let new_git_dir = git_dir_of(&new_root);
    let (st, repo) = http_patch(
        &server.url(&format!("/api/repos/{repo_id}")),
        &json!({"git_dir": new_git_dir}),
    );
    assert_eq!(st, 200, "{repo}");
    assert_eq!(repo["git_dir"].as_str().unwrap(), new_git_dir);
    assert_eq!(repo["id"].as_u64(), Some(repo_id));
    assert_eq!(repo["base_branch"], "main", "base survives a relocation");

    // GET /api/repos now reports the new path for the same id.
    let (_, list) = http_get(&server.url("/api/repos"));
    let row = list["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_u64() == Some(repo_id))
        .unwrap();
    assert_eq!(row["git_dir"].as_str().unwrap(), new_git_dir);
}

#[test]
fn get_repo_by_id_endpoint() {
    let a = GitRepo::new();
    let a1 = a.commit(&[a.root], &msg("a: one", "Ia1"), &[("a.rs", "a\n")]);
    a.branch("feat", a1);
    let server = TestServer::start(a.dir.path().join("nit.sqlite3"), None);
    let (st, _) = push(&server, &a, "feat", "main", None);
    assert_eq!(st, 200);
    let repo_id = first_repo(&server);

    // The by-id repo carries the same shape the list reports for that row.
    let (st, repo) = http_get(&server.url(&format!("/api/repos/{repo_id}")));
    assert_eq!(st, 200, "{repo}");
    assert_eq!(repo["id"].as_u64(), Some(repo_id));
    assert_eq!(repo["git_dir"].as_str().unwrap(), a.git_dir());
    assert_eq!(repo["base_branch"], "main");
    assert_eq!(repo["active_chains"].as_u64(), Some(1));

    // Unknown id → 404.
    let (st, _) = http_get(&server.url("/api/repos/9999"));
    assert_eq!(st, 404);
}

#[test]
fn nit_repo_move_cli() {
    let a = GitRepo::new();
    let a1 = a.commit(&[a.root], &msg("a: one", "Ia1"), &[("a.rs", "a\n")]);
    a.branch("feat", a1);
    let server = TestServer::start(a.dir.path().join("nit.sqlite3"), None);
    let (st, _) = push(&server, &a, "feat", "main", None);
    assert_eq!(st, 200);
    let old_git_dir = a.git_dir();

    // Move the repo, then `nit repo move <old git dir> <new root>`. The cwd is
    // the still-present tempdir — the command keys off its args, not the cwd.
    let new_root = a.dir.path().join("moved");
    std::fs::rename(a.dir.path().join("repo"), &new_root).unwrap();
    let new_git_dir = git_dir_of(&new_root);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["repo", "move", &old_git_dir, new_root.to_str().unwrap()])
        .current_dir(a.dir.path())
        .env("NIT_SERVER", &server.base)
        .output()
        .expect("running nit repo move");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let updated: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(updated["git_dir"].as_str().unwrap(), new_git_dir);

    // `nit repo list` (≡ GET /api/repos) now shows the new path.
    let (_, list) = http_get(&server.url("/api/repos"));
    assert_eq!(list["repos"][0]["git_dir"].as_str().unwrap(), new_git_dir);
}

#[test]
fn merged_chain_drops_out_of_active_chains() {
    fast_timer();
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: one", "Im1"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, _) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200);
    let id = first_repo(&server);
    assert_eq!(active_chains(&server, id), 1, "one live tip after the push");

    // Land the change on the canonical branch (a fast-forward of main onto the
    // tip). The background timer detects the patch-id on `main` and marks the
    // change merged, so the chain leaves the live-tip set.
    g.branch("main", c1);
    wait_for(Duration::from_secs(5), || {
        (active_chains(&server, id) == 0).then_some(())
    });
}

#[test]
fn abandoned_chain_drops_out_of_active_chains() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: one", "Iab1"), &[("a.rs", "a\n")]);
    g.branch("feat", c1);

    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "feat", "main", None);
    assert_eq!(st, 200);
    let change_id = member_id(&res, "Iab1");
    let id = first_repo(&server);
    assert_eq!(active_chains(&server, id), 1);

    // Abandon the change: it drops out of the active-tip set (the dashboard
    // hides abandoned tips), even though it stays enumerable as its own chain.
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/abandon")),
        &json!({}),
    );
    assert_eq!(st, 200);
    assert_eq!(active_chains(&server, id), 0);
}

// ---------------------------------------------------------------------------
// Small repo-registry helpers (one repo per server in these tests).

/// The only registered repo's id.
fn first_repo(server: &TestServer) -> u64 {
    let (_, list) = http_get(&server.url("/api/repos"));
    list["repos"][0]["id"].as_u64().expect("a repo")
}

/// A repo's recorded canonical `base_branch`.
fn repo_base(server: &TestServer, id: u64) -> String {
    let (_, list) = http_get(&server.url("/api/repos"));
    list["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_u64() == Some(id))
        .unwrap_or_else(|| panic!("repo {id} missing"))
        .get("base_branch")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no base_branch on repo {id}"))
        .to_string()
}
