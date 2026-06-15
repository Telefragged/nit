//! The repo registry over HTTP: `GET /api/repos` lists each repo with its
//! active-chain count, and `GET /api/chains?repo={id}` scopes the chain list
//! to one repo (docs/api.md "Repos"). Two worktrees/branches of one repo
//! share a single registry row, keyed by the git-common-dir.

mod common;

use common::{GitRepo, TestServer, http_get, http_post, msg};
use serde_json::json;

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
