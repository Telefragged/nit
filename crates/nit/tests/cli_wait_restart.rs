//! `nit wait` across a server restart: the parked long-poll returns
//! promptly on shutdown, the CLI retries through the outage instead of
//! dying, and the persisted event cursor resumes on the restarted
//! server — covering classification, backoff, cursor validity and
//! stdout purity in one pass (docs/agent-workflow.md).

mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{GitRepo, TestServer, http_post, msg, nit};
use serde_json::json;

#[test]
fn wait_survives_a_server_restart() {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("core: add a", "Ia"), &[("a.txt", "a\nb\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let db = g.dir.path().join("nit.sqlite3");
    let server = TestServer::start(db.clone(), None);
    let addr = server.addr;

    let (ok, chain, stderr) = nit(&server, &g, &["push"]);
    assert!(ok, "{stderr}");
    let change_id = chain["changes"][0]["id"].as_i64().unwrap();

    // Park the real binary on `nit wait` (--timeout 30 is the hang guard)
    // and give it time to resolve the chain and enter the long-poll.
    let mut child = Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(["wait", "--timeout", "30"])
        .current_dir(g.workdir())
        .env("NIT_SERVER", &server.base)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));

    // Shutdown must be prompt end-to-end: the parked poll returns early
    // instead of holding the drop for its full poll timeout.
    let dropping = Instant::now();
    drop(server);
    let dropped_in = dropping.elapsed();
    assert!(dropped_in < Duration::from_secs(4), "{dropped_in:?}");

    // The outage: wait must retry, not die on the refused connection.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        child.try_wait().unwrap().is_none(),
        "nit wait died during the outage"
    );

    // Same address, same db: chain id and event cursor stay valid.
    let server = TestServer::start_at(addr, db, None);
    let (st, _) = http_post(
        &server.url(&format!("/api/changes/{change_id}/reviews")),
        &json!({"revision": 1, "verdict": "request_changes", "message": "fix it"}),
    );
    assert_eq!(st, 200);

    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let feedback: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must stay pure JSON");
    assert_eq!(feedback["state"], "agents_turn", "{feedback}");
    assert_eq!(feedback["actionable"], true);
    assert!(stderr.contains("retrying"), "{stderr}");
}
