//! The repo registry over HTTP: `GET /api/repos` lists each repo with its
//! active-chain count, `GET /api/chains?repo={id}` scopes the chain list to
//! one repo, and `PATCH /api/repos/{id}` (≡ `nit repo move`) repoints a repo
//! after a disk move (docs/api.md "Repos"). Two worktrees/branches of one
//! repo share a single registry row, keyed by the git-common-dir.

mod common;

use common::{GitRepo, TestServer, http_get, http_patch, http_post, msg};
use serde_json::{Value, json};

/// Canonical git-common-dir of a git checkout rooted at `root` (a `.git`
/// child), as a string — the repo identity nit stores.
fn git_dir_of(root: &std::path::Path) -> String {
    std::fs::canonicalize(root.join(".git"))
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

fn register(server: &TestServer, g: &GitRepo, branch: &str) -> serde_json::Value {
    let (st, chain) = http_post(
        &server.url("/api/chains"),
        &json!({"git_dir": g.git_dir(), "branch": branch, "base": "main"}),
    );
    assert_eq!(st, 200, "{chain}");
    chain
}

#[test]
fn repos_list_and_scoped_chains() {
    // Two distinct repos (distinct git dirs); the second carries two chains.
    let a = GitRepo::new();
    let a1 = a.commit(&[a.root], &msg("a: one", "Ia1"), &[("a.rs", "a\n")]);
    a.branch("feat", a1);

    let b = GitRepo::new();
    let b1 = b.commit(&[b.root], &msg("b: one", "Ib1"), &[("b.rs", "b\n")]);
    b.branch("feat", b1);
    let b2 = b.commit(&[b1], &msg("b: two", "Ib2"), &[("b2.rs", "b2\n")]);
    b.branch("topic", b2);

    let server = TestServer::start(a.dir.path().join("nit.sqlite3"), None);

    let ca = register(&server, &a, "feat");
    let cb1 = register(&server, &b, "feat");
    let cb2 = register(&server, &b, "topic");

    let repo_a = ca["repo_id"].as_u64().unwrap();
    let repo_b = cb1["repo_id"].as_u64().unwrap();
    assert_ne!(repo_a, repo_b, "distinct git dirs are distinct repos");
    assert_eq!(
        cb2["repo_id"].as_u64().unwrap(),
        repo_b,
        "two branches of one repo share its registry row"
    );

    // GET /api/repos: both repos, each with its active-chain count.
    let (st, list) = http_get(&server.url("/api/repos"));
    assert_eq!(st, 200);
    let repos = list["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 2);
    let by_id = |id: u64| {
        repos
            .iter()
            .find(|r| r["id"].as_u64() == Some(id))
            .unwrap_or_else(|| panic!("repo {id} missing from {list}"))
    };
    assert_eq!(by_id(repo_a)["git_dir"].as_str().unwrap(), a.git_dir());
    assert_eq!(by_id(repo_a)["active_chains"].as_u64(), Some(1));
    assert_eq!(by_id(repo_b)["active_chains"].as_u64(), Some(2));

    // GET /api/chains?repo=: scoped to one repo's chains only.
    let (st, scoped) = http_get(&server.url(&format!("/api/chains?repo={repo_b}")));
    assert_eq!(st, 200);
    let b_chains = scoped["chains"].as_array().unwrap();
    assert_eq!(b_chains.len(), 2);
    assert!(
        b_chains
            .iter()
            .all(|c| c["repo_id"].as_u64() == Some(repo_b))
    );

    let (_, scoped_a) = http_get(&server.url(&format!("/api/chains?repo={repo_a}")));
    let a_chains = scoped_a["chains"].as_array().unwrap();
    assert_eq!(a_chains.len(), 1);
    assert_eq!(a_chains[0]["git_dir"].as_str().unwrap(), a.git_dir());
}

#[test]
fn relocate_repo_endpoint() {
    let a = GitRepo::new();
    let a1 = a.commit(&[a.root], &msg("a: one", "Ia1"), &[("a.rs", "a\n")]);
    a.branch("feat", a1);
    let server = TestServer::start(a.dir.path().join("nit.sqlite3"), None);
    let chain = register(&server, &a, "feat");
    let repo_id = chain["repo_id"].as_u64().unwrap();
    let chain_id = chain["id"].as_i64().unwrap();

    // Unknown repo → 404; an unresolvable new path → 400.
    let (st, _) = http_patch(
        &server.url("/api/repos/999"),
        &json!({"git_dir": a.git_dir()}),
    );
    assert_eq!(st, 404);
    let (st, _) = http_patch(
        &server.url(&format!("/api/repos/{repo_id}")),
        &json!({"git_dir": "/does/not/exist/.git"}),
    );
    assert_eq!(st, 400);

    // Move the repo on disk, then repoint it at the new git dir.
    let new_root = a.dir.path().join("moved");
    std::fs::rename(a.dir.path().join("repo"), &new_root).unwrap();
    let new_git_dir = git_dir_of(&new_root);
    let (st, repo) = http_patch(
        &server.url(&format!("/api/repos/{repo_id}")),
        &json!({"git_dir": new_git_dir}),
    );
    assert_eq!(st, 200, "{repo}");
    assert_eq!(repo["git_dir"].as_str().unwrap(), new_git_dir);

    // The existing chain scans cleanly at the new path (same chain, no error).
    let (st, refreshed) = http_post(
        &server.url("/api/chains"),
        &json!({"git_dir": new_git_dir, "branch": "feat", "base": "main"}),
    );
    assert_eq!(st, 200, "{refreshed}");
    assert_eq!(refreshed["id"].as_i64().unwrap(), chain_id);
    assert_eq!(refreshed["last_scan_error"], Value::Null);
    assert_eq!(refreshed["git_dir"].as_str().unwrap(), new_git_dir);
}

#[test]
fn nit_repo_move_cli() {
    let a = GitRepo::new();
    let a1 = a.commit(&[a.root], &msg("a: one", "Ia1"), &[("a.rs", "a\n")]);
    a.branch("feat", a1);
    let server = TestServer::start(a.dir.path().join("nit.sqlite3"), None);
    let chain = register(&server, &a, "feat");
    let old_git_dir = chain["git_dir"].as_str().unwrap().to_string();

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
    let updated: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(updated["git_dir"].as_str().unwrap(), new_git_dir);

    // `nit repo list` (≡ GET /api/repos) now shows the new path.
    let (_, list) = http_get(&server.url("/api/repos"));
    assert_eq!(list["repos"][0]["git_dir"].as_str().unwrap(), new_git_dir);
}
