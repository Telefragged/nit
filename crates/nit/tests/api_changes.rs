//! The bulk change read (`GET /api/changes`): the explicit `status` filter —
//! repeatable, matched at each change's latest revision, and **absent means
//! every change** (the API bakes in no default subset).

mod common;

use common::{
    GitRepo, TestServer, change_by_label, change_id, change_ids, first_repo_id, get_changes, msg,
    push, review,
};

// Two single-change chains, one reviewed: no filter returns both full
// projections, a status filter selects by each change's latest-revision
// status, and repeated params union.
#[test]
fn status_filter_is_explicit_and_absent_means_all() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic-a", a);
    let b = g.commit(&[g.root], &msg("b: two", "Ib"), &[("b", "2\n")]);
    g.branch("topic-b", b);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);

    let (st, res) = push(&server, &g, "topic-a", "main");
    assert_eq!(st, 200, "{res}");
    let (st, res) = push(&server, &g, "topic-b", "main");
    assert_eq!(st, 200, "{res}");
    let repo_id = first_repo_id(&server);

    let all = get_changes(&server, &format!("?repo={repo_id}"));
    assert_eq!(
        change_ids(&all),
        vec![change_id("Ia"), change_id("Ib")],
        "no filter: every change"
    );
    // The payload is the folded projection, not a summary shape.
    assert!(
        all.iter().all(|c| c["revisions"].is_array()),
        "ChangeProjection payload: {all:?}"
    );

    let ia = change_by_label(&server, repo_id, "Ia");
    review(&server, ia["id"].as_u64().expect("id"), "approve", "lgtm");

    let approved = get_changes(&server, &format!("?repo={repo_id}&status=approved"));
    assert_eq!(change_ids(&approved), vec![change_id("Ia")]);
    let pending = get_changes(&server, &format!("?repo={repo_id}&status=pending"));
    assert_eq!(change_ids(&pending), vec![change_id("Ib")]);
    let both = get_changes(
        &server,
        &format!("?repo={repo_id}&status=approved&status=pending"),
    );
    assert_eq!(
        change_ids(&both),
        vec![change_id("Ia"), change_id("Ib")],
        "repeated params union"
    );
    let merged = get_changes(&server, &format!("?repo={repo_id}&status=merged"));
    assert!(merged.is_empty(), "{merged:?}");
}

// An unknown repo matches nothing — an empty list, not a 404 (exactly as
// `/api/chains`).
#[test]
fn unknown_repo_filters_to_empty() {
    let g = GitRepo::new();
    let a = g.commit(&[g.root], &msg("a: one", "Ia"), &[("a", "1\n")]);
    g.branch("topic-a", a);
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (st, res) = push(&server, &g, "topic-a", "main");
    assert_eq!(st, 200, "{res}");

    assert!(get_changes(&server, "?repo=999").is_empty());
}
