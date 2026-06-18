//! Shared integration-test harness: a tiny real git repo (built with git2, no
//! worktree needed) and a real `nit::api` server on port 0, with blocking HTTP
//! helpers. A chain is derived, never registered — tests drive `POST /api/push`
//! and read the on-demand chain endpoints.
//!
//! Each integration-test binary compiles its own copy, so helpers unused by one
//! binary are fine.
#![expect(
    dead_code,
    reason = "each test binary compiles its own copy and uses a subset"
)]

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use git2::{Oid, Repository, RepositoryInitOptions, Signature, Time};
use serde_json::{Value, json};

/// Strictly increasing commit timestamps so equal-content commits get distinct
/// shas.
static CLOCK: AtomicI64 = AtomicI64::new(1_700_000_000);

pub fn sig() -> Signature<'static> {
    let t = CLOCK.fetch_add(1, Ordering::SeqCst);
    Signature::new("Test", "test@example.com", &Time::new(t, 0)).unwrap()
}

/// Configure a fast lifecycle timer for the whole test process (one timer
/// interval, one abandon window). Call once before starting a server in a
/// lifecycle test. Process-global, so a test file relying on it must use the
/// same values throughout.
pub fn fast_timer() {
    // SAFETY: set before any server thread reads it; identical values across a
    // file's tests, so concurrent writers do not tear.
    unsafe {
        std::env::set_var("NIT_TIMER_INTERVAL_MS", "150");
        std::env::set_var("NIT_ABANDON_SECS", "1");
    }
}

/// A standalone fixture repo: `main` with one root commit.
pub struct GitRepo {
    pub dir: tempfile::TempDir,
    pub repo: Repository,
    pub root: Oid,
}

impl GitRepo {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("refs/heads/main");
        let repo = Repository::init_opts(dir.path().join("repo"), &opts).unwrap();
        let root = commit_in(&repo, &[], "init\n", &[("README", "hello\n")]);
        repo.reference("refs/heads/main", root, true, "test")
            .unwrap();
        GitRepo { dir, repo, root }
    }

    pub fn workdir(&self) -> std::path::PathBuf {
        self.repo.workdir().unwrap().to_path_buf()
    }

    /// The repo's canonical git-common-dir — the repo identity on the wire.
    pub fn git_dir(&self) -> String {
        git_dir_string(&self.repo)
    }

    pub fn commit(&self, parents: &[Oid], message: &str, files: &[(&str, &str)]) -> Oid {
        commit_in(&self.repo, parents, message, files)
    }

    pub fn commit_full(
        &self,
        parents: &[Oid],
        message: &str,
        upserts: &[(&str, &[u8])],
        deletes: &[&str],
    ) -> Oid {
        commit_full_in(&self.repo, parents, message, upserts, deletes)
    }

    pub fn branch(&self, name: &str, target: Oid) {
        self.repo
            .reference(&format!("refs/heads/{name}"), target, true, "test")
            .unwrap();
    }

    pub fn delete_branch(&self, name: &str) {
        self.repo
            .find_reference(&format!("refs/heads/{name}"))
            .unwrap()
            .delete()
            .unwrap();
    }

    pub fn tip(&self, name: &str) -> Oid {
        self.repo
            .find_reference(&format!("refs/heads/{name}"))
            .unwrap()
            .target()
            .unwrap()
    }
}

/// A repo's canonical git-common-dir as a string.
fn git_dir_string(repo: &Repository) -> String {
    std::fs::canonicalize(repo.commondir())
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

fn commit_in(repo: &Repository, parents: &[Oid], message: &str, files: &[(&str, &str)]) -> Oid {
    let upserts: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(path, content)| (*path, content.as_bytes()))
        .collect();
    commit_full_in(repo, parents, message, &upserts, &[])
}

fn commit_full_in(
    repo: &Repository,
    parents: &[Oid],
    message: &str,
    upserts: &[(&str, &[u8])],
    deletes: &[&str],
) -> Oid {
    let parent_commits: Vec<git2::Commit> = parents
        .iter()
        .map(|&oid| repo.find_commit(oid).unwrap())
        .collect();
    let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();
    let mut index = git2::Index::new().unwrap();
    if let Some(parent) = parent_commits.first() {
        index.read_tree(&parent.tree().unwrap()).unwrap();
    }
    for (path, content) in upserts {
        let entry = git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o100_644,
            uid: 0,
            gid: 0,
            file_size: u32::try_from(content.len()).unwrap(),
            id: repo.blob(content).unwrap(),
            flags: 0,
            flags_extended: 0,
            path: path.as_bytes().to_vec(),
        };
        index.add(&entry).unwrap();
    }
    for path in deletes {
        index.remove_path(std::path::Path::new(path)).unwrap();
    }
    let tree_oid = index.write_tree_to(repo).unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let s = sig();
    repo.commit(None, &s, &s, message, &tree, &parent_refs)
        .unwrap()
}

// ---------------------------------------------------------------------------
// Server harness

/// A real `nit::api` server (the binary's stack) bound on port 0.
pub struct TestServer {
    pub base: String,
    pub addr: std::net::SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    served: Option<tokio::task::JoinHandle<()>>,
    rt: Option<tokio::runtime::Runtime>,
}

impl TestServer {
    pub fn start(db_path: std::path::PathBuf, web_dist: Option<std::path::PathBuf>) -> Self {
        Self::start_at("127.0.0.1:0".parse().unwrap(), db_path, web_dist)
    }

    pub fn start_at(
        addr: std::net::SocketAddr,
        db_path: std::path::PathBuf,
        web_dist: Option<std::path::PathBuf>,
    ) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind(addr)).unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let served = rt.spawn(async move {
            nit::api::serve_on(listener, db_path, web_dist, async {
                let _ = rx.await;
            })
            .await
            .unwrap();
        });
        TestServer {
            base,
            addr,
            shutdown: Some(tx),
            served: Some(served),
            rt: Some(rt),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(rt) = self.rt.take() {
            if let Some(served) = self.served.take() {
                let _ = rt
                    .block_on(async { tokio::time::timeout(Duration::from_secs(5), served).await });
            }
            rt.shutdown_timeout(Duration::from_secs(5));
        }
    }
}

/// Run the real `nit` binary (`CARGO_BIN_EXE`) from inside `repo` against
/// `server`: (exit ok, parsed stdout JSON, stderr).
pub fn nit(server: &TestServer, repo: &GitRepo, args: &[&str]) -> (bool, Value, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(args)
        .current_dir(repo.workdir())
        .env("NIT_SERVER", &server.base)
        .output()
        .expect("running nit");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let value = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    (out.status.success(), value, stderr)
}

/// `nit push` / `nit ready` with the repo path and branch passed explicitly.
/// `cmd` is `"push"` or `"ready"`; `extra` carries flags like `--partial`.
pub fn nit_register(
    server: &TestServer,
    repo: &GitRepo,
    cmd: &str,
    branch: &str,
    extra: &[&str],
) -> (bool, Value, String) {
    let workdir = repo.workdir();
    let workdir = workdir.to_str().expect("workdir path is valid UTF-8");
    let mut args = vec![cmd, "--repo", workdir, "--branch", branch];
    args.extend_from_slice(extra);
    nit(server, repo, &args)
}

fn agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

fn read(mut response: ureq::http::Response<ureq::Body>) -> (u16, Value) {
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap();
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    (status, value)
}

pub fn http_get(url: &str) -> (u16, Value) {
    read(agent().get(url).call().unwrap())
}

pub fn http_post(url: &str, body: &Value) -> (u16, Value) {
    read(agent().post(url).send_json(body).unwrap())
}

pub fn http_patch(url: &str, body: &Value) -> (u16, Value) {
    read(agent().patch(url).send_json(body).unwrap())
}

pub fn http_delete(url: &str) -> (u16, Value) {
    read(agent().delete(url).call().unwrap())
}

// ---------------------------------------------------------------------------
// Change-centric helpers (push + chain navigation)

/// `POST /api/push` over HTTP. `tip` is a branch name or sha; `partial`
/// optionally sets/clears the sticky flag. Returns `(status, PushResult)`.
pub fn push(
    server: &TestServer,
    repo: &GitRepo,
    tip: &str,
    base: &str,
    partial: Option<bool>,
) -> (u16, Value) {
    let mut body = json!({"git_dir": repo.git_dir(), "tip": tip, "base": base});
    if let Some(p) = partial {
        body["partial"] = json!(p);
    }
    http_post(&server.url("/api/push"), &body)
}

/// The first registered repo's id.
pub fn first_repo_id(server: &TestServer) -> u64 {
    let (_, repos) = http_get(&server.url("/api/repos"));
    repos["repos"][0]["id"].as_u64().expect("a repo")
}

/// Find a path member's `change_id` by its Change-Id in a `Chain`/`PushResult`
/// path (`value["path"]` or `value["chain"]["path"]`).
pub fn member_id(value: &Value, change_key: &str) -> u64 {
    let path = value
        .get("path")
        .or_else(|| value.get("chain").and_then(|c| c.get("path")))
        .and_then(|p| p.as_array())
        .expect("a path");
    path.iter()
        .find(|m| m["change_key"].as_str() == Some(change_key))
        .and_then(|m| m["change_id"].as_u64())
        .unwrap_or_else(|| panic!("no member {change_key} in path"))
}

/// `subject` + `Change-Id` trailer message.
pub fn msg(subject: &str, change_id: &str) -> String {
    format!("{subject}\n\nChange-Id: {change_id}\n")
}

/// Poll `predicate` until it returns `Some`, or panic after `timeout` — for
/// the asynchronous lifecycle timer.
pub fn wait_for<T>(timeout: Duration, mut predicate: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = predicate() {
            return v;
        }
        assert!(
            Instant::now() < deadline,
            "condition not met within {timeout:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// Websocket change stream (WS /api/stream)

pub type WsSock = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

/// Connect `WS /api/stream` and `subscribe` the given changes (`change_id` →
/// from-idx), with a read timeout so `ws_read` never blocks the test forever.
pub fn ws_subscribe(server: &TestServer, subs: &[(u64, u64)], read_timeout: Duration) -> WsSock {
    let url = format!("ws://{}/api/stream", server.addr);
    let (mut socket, _) = tungstenite::connect(&url).expect("ws connect");
    if let tungstenite::stream::MaybeTlsStream::Plain(s) = socket.get_ref() {
        s.set_read_timeout(Some(read_timeout))
            .expect("read timeout");
    }
    let map: std::collections::HashMap<String, u64> =
        subs.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let sub = json!({ "subscribe": map }).to_string();
    socket
        .send(tungstenite::Message::Text(sub.into()))
        .expect("subscribe");
    socket
}

/// The next Text frame parsed as JSON, or `None` on read timeout / close.
pub fn ws_read(socket: &mut WsSock) -> Option<Value> {
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(t)) => return serde_json::from_str(t.as_str()).ok(),
            Ok(tungstenite::Message::Ping(p)) => {
                let _ = socket.send(tungstenite::Message::Pong(p));
            }
            Ok(_) => {}
            Err(_) => return None, // timeout or close
        }
    }
}

/// Run the `nit` binary with a hard deadline — a `wait`/`--follow` that never
/// wakes is killed and reported, never hangs the suite.
pub fn nit_bounded(
    server: &TestServer,
    repo: &GitRepo,
    args: &[&str],
    deadline: Duration,
) -> (bool, Value, String) {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(args)
        .current_dir(repo.workdir())
        .env("NIT_SERVER", &server.base)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn nit");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            let out = child.wait_with_output().expect("output");
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            let value = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
            return (out.status.success(), value, stderr);
        }
        if start.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("nit {args:?} did not finish within {deadline:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
