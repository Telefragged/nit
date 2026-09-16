//! Shared integration-test harness: a tiny real git repo (built with git2, no
//! worktree needed) and a real `nit::api` server on port 0, with blocking HTTP
//! helpers. Tests drive `POST /api/push` and read the change endpoints.
//!
//! Each integration-test binary compiles its own copy, so helpers unused by one
//! binary are fine.
#![expect(
    dead_code,
    reason = "each test binary compiles its own copy and uses a subset"
)]

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use git2::{Oid, Repository, RepositoryInitOptions, Signature, Time};
use serde_json::{Value, json};

/// Strictly increasing commit timestamps so equal-content commits get distinct
/// shas.
static CLOCK: AtomicI64 = AtomicI64::new(1_700_000_000);

/// `prefix1\nprefix2\n…` over `n`: a file body with one numbered line each.
pub fn lines(prefix: &str, n: std::ops::RangeInclusive<i64>) -> String {
    use std::fmt::Write;
    n.fold(String::new(), |mut s, i| {
        writeln!(s, "{prefix}{i}").unwrap();
        s
    })
}

pub fn sig() -> Signature<'static> {
    let t = CLOCK.fetch_add(1, Ordering::SeqCst);
    Signature::new("Test", "test@example.com", &Time::new(t, 0)).unwrap()
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

    /// The canonical workdir path, as the `worktree` tag spells it.
    pub fn canonical_workdir(&self) -> String {
        let workdir = std::fs::canonicalize(self.workdir()).expect("canonical workdir");
        workdir.to_str().expect("utf-8 workdir").to_owned()
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

/// A real `nit::api` server (the binary's stack) bound on port 0. The harness
/// owns the `AppState` the server runs on so a test can drive a lifecycle
/// sweep in-process (`sweep`); no background timer runs.
pub struct TestServer {
    pub base: String,
    pub addr: std::net::SocketAddr,
    state: Option<std::sync::Arc<nit::api::AppState>>,
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
        let state = rt.block_on(nit::api::AppState::load(db_path)).unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let served = {
            let state = state.clone();
            rt.spawn(async move {
                nit::api::serve_on_state(listener, state, web_dist, async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
            })
        };
        TestServer {
            base,
            addr,
            state: Some(state),
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
            // Drop the pooled AppState inside the runtime: deadpool closes
            // sqlite connections via spawn_blocking, which needs a live runtime.
            {
                let _enter = rt.enter();
                drop(self.state.take());
            }
            rt.shutdown_timeout(Duration::from_secs(5));
        }
    }
}

/// Run the real `nit` binary (`CARGO_BIN_EXE`) from inside `repo` against
/// `server`: (exit ok, parsed stdout JSON, stderr).
pub fn nit(server: &TestServer, repo: &GitRepo, args: &[&str]) -> (bool, Value, String) {
    nit_env(server, repo, args, &[])
}

/// [`nit`] with `envs` set on the child.
///
/// The harness clears `CLAUDE_CODE_SESSION_ID` first, because `nit push`
/// derives a `session-id` tag from it. A test run under Claude Code would
/// otherwise see a tag set that depends on who ran it.
pub fn nit_env(
    server: &TestServer,
    repo: &GitRepo,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (bool, Value, String) {
    let out = nit_command(server, repo, args)
        .envs(envs.iter().copied())
        .output()
        .expect("running nit");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout);
    (out.status.success(), parse_stdout(&stdout), stderr)
}

/// The `nit` child every helper runs, configured but not spawned.
fn nit_command(server: &TestServer, repo: &GitRepo, args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_nit"));
    cmd.args(args)
        .current_dir(repo.workdir())
        .env("NIT_SERVER", &server.base)
        .env_remove("CLAUDE_CODE_SESSION_ID");
    cmd
}

/// The child's stdout as JSON, or as a string when it is not JSON.
///
/// Text-output commands (status/push/comment/…) aren't JSON, and keeping
/// their raw stdout lets a test assert on the rendered lines.
fn parse_stdout(stdout: &str) -> Value {
    serde_json::from_str(stdout.trim()).unwrap_or_else(|_| Value::String(stdout.trim().to_string()))
}

/// A bare `nit push` from inside the repo: the checked-out commit, tagged
/// with its branch. Registers the repo first (`nit repo create
/// --canonical-ref main`) so the push has a base to fork from; a repeat
/// create just errors, which is ignored.
pub fn nit_register(server: &TestServer, repo: &GitRepo) -> (bool, Value, String) {
    let _ = nit(server, repo, &["repo", "create", "--canonical-ref", "main"]);
    nit(server, repo, &["push"])
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

pub fn http_put(url: &str, body: &Value) -> (u16, Value) {
    read(agent().put(url).send_json(body).unwrap())
}

pub fn http_delete(url: &str) -> (u16, Value) {
    read(agent().delete(url).call().unwrap())
}

/// `POST /api/repos` over HTTP (≡ `nit repo create`). `base` pins the canonical
/// canonical ref (any git ref that resolves to a commit). Returns `(status, Repo)`.
pub fn create_repo(server: &TestServer, repo: &GitRepo, canonical_ref: &str) -> (u16, Value) {
    let body = json!({"git_dir": repo.git_dir(), "canonical_ref": canonical_ref});
    http_post(&server.url("/api/repos"), &body)
}

/// `POST /api/push` over HTTP, registering the repo first (`create_repo` with
/// `canonical_ref`). `tip` is a branch name or sha. A
/// failing registration other than "already registered" (409) is returned
/// as-is. Returns `(status, PushResult)`.
pub fn push(server: &TestServer, repo: &GitRepo, tip: &str, base: &str) -> (u16, Value) {
    push_body(
        server,
        repo,
        base,
        &json!({"git_dir": repo.git_dir(), "tip": tip}),
    )
}

/// `POST /api/changes/{number}/tags` (≡ `nit push --tag key=value`).
///
/// `tags` is the request's `key: value` object.
pub fn tag_change(server: &TestServer, change_number: u64, tags: &Value) -> (u16, Value) {
    http_post(
        &server.url(&format!("/api/changes/{change_number}/tags")),
        &json!({ "tags": tags }),
    )
}

/// The change `tip` registers, as a fixture for a test about something
/// other than pushing.
///
/// A change only comes into being through a push, so a test that needs
/// one starts here.
pub fn a_change(server: &TestServer, repo: &GitRepo, tip: &str) -> u64 {
    let (st, res) = push(server, repo, tip, "main");
    assert_eq!(st, 200, "{res}");
    tip_change(&res)["change_number"]
        .as_u64()
        .expect("tip change number")
}

/// The tip of a `PushResult`: the last change the push walked.
pub fn tip_change(res: &Value) -> &Value {
    res["changes"]
        .as_array()
        .and_then(|changes| changes.last())
        .expect("a PushResult `changes`")
}

/// Registers the repo, then posts `body` to `/api/push`.
fn push_body(server: &TestServer, repo: &GitRepo, base: &str, body: &Value) -> (u16, Value) {
    let (st, created) = create_repo(server, repo, base);
    if st != 200 && st != 409 {
        return (st, created);
    }
    http_post(&server.url("/api/push"), body)
}

/// The changes `GET /api/changes{query}` returns.
pub fn get_changes(server: &TestServer, query: &str) -> Vec<Value> {
    let (st, body) = http_get(&server.url(&format!("/api/changes{query}")));
    assert_eq!(st, 200, "{body}");
    body["changes"].as_array().expect("changes array").clone()
}

/// The `Change-Id`s of `changes`, sorted.
pub fn change_ids(changes: &[Value]) -> Vec<String> {
    let mut ids: Vec<String> = changes
        .iter()
        .map(|c| c["change_id"].as_str().expect("change_id").to_string())
        .collect();
    ids.sort();
    ids
}

/// One repo's change, by the label its `Change-Id` expands from.
pub fn change_by_label(server: &TestServer, repo_id: u64, label: &str) -> Value {
    let key = change_id(label);
    get_changes(server, &format!("?repo={repo_id}"))
        .into_iter()
        .find(|c| c["change_id"] == key)
        .unwrap_or_else(|| panic!("no change {label} in repo {repo_id}"))
}

/// One change's tags, by label.
///
/// Absent when the change carries none, because `skip_serializing_if`
/// omits an empty set rather than shipping `{}`.
pub fn change_tags(server: &TestServer, repo_id: u64, label: &str) -> Value {
    change_by_label(server, repo_id, label)["tags"].clone()
}

/// Publish a verdict on a change through the only publish path — draft the
/// decision, then batch-submit that one change. Returns the
/// `BatchSubmitResult`.
pub fn review(server: &TestServer, change_number: u64, verdict: &str, message: &str) -> Value {
    let (st, _) = http_put(
        &server.url(&format!("/api/changes/{change_number}/decision")),
        &json!({"decision": verdict, "message": message}),
    );
    assert_eq!(st, 200, "draft decision on change {change_number}");
    submit_change(server, change_number)
}

/// Submits the one change, by its `Change-Id`.
pub fn submit_change(server: &TestServer, change_number: u64) -> Value {
    let (st, detail) = http_get(&server.url(&format!("/api/changes/{change_number}")));
    assert_eq!(st, 200, "{detail}");
    let change_id = detail["change_id"].as_str().expect("a change id");
    submit(server, &format!("change_id={change_id}"))
}

/// `POST /api/submit?{query}`: publishes the draft decisions of the changes
/// the query picks. Returns the `BatchSubmitResult`.
pub fn submit(server: &TestServer, query: &str) -> Value {
    let (st, out) = http_post(&server.url(&format!("/api/submit?{query}")), &json!({}));
    assert_eq!(st, 200, "submit {query}: {out}");
    out
}

/// A change's status at `revision`, or at its latest revision for `None`.
///
/// `None` back means the change or the revision does not exist.
pub fn status_at(server: &TestServer, change_number: u64, revision: Option<u64>) -> Option<String> {
    let (st, detail) = http_get(&server.url(&format!("/api/changes/{change_number}")));
    if st != 200 {
        return None;
    }
    let revisions = detail["revisions"].as_array()?;
    let revision = match revision {
        Some(number) => revisions.get(usize::try_from(number).ok()?)?,
        None => revisions.last()?,
    };
    revision["status"].as_str().map(str::to_string)
}

pub fn first_repo_id(server: &TestServer) -> u64 {
    let (_, repos) = http_get(&server.url("/api/repos"));
    repos["repos"][0]["id"].as_u64().expect("a repo")
}

/// Every entry of the first repo's log, in sequence order.
pub fn repo_log(server: &TestServer) -> Vec<Value> {
    let repo_id = first_repo_id(server);
    let (st, log) = http_get(&server.url(&format!("/api/log?repo={repo_id}")));
    assert_eq!(st, 200, "{log}");
    log["entries"].as_array().expect("entries").clone()
}

/// Find a change's number by its Change-Id in a `PushResult`.
pub fn member_id(value: &Value, label: &str) -> u64 {
    let key = change_id(label);
    value["changes"]
        .as_array()
        .expect("a PushResult `changes`")
        .iter()
        .find(|m| m["change_id"].as_str() == Some(key.as_str()))
        .and_then(|m| m["change_number"].as_u64())
        .unwrap_or_else(|| panic!("no change {label} in the push result"))
}

pub fn msg(subject: &str, label: &str) -> String {
    format!("{subject}\n\nChange-Id: {}\n", change_id(label))
}

/// A distinct, well-formed `Change-Id` for `label`, as the wire spells it.
pub fn change_id(label: &str) -> String {
    nit_types::testing::change_id(label).to_string()
}

/// Drive one lifecycle sweep synchronously, in-process, against the server's
/// own `AppState` — deterministic merge detection with no timer and no HTTP
/// round-trip. Returns once the sweep has committed.
pub fn sweep(server: &TestServer) {
    let state = server.state.as_ref().expect("state");
    server
        .rt
        .as_ref()
        .expect("runtime")
        .block_on(nit::api::sweep_once(state));
}

/// How long the harness waits before it calls the suite hung: a child that
/// never exits, or a frame that never arrives. Only a broken test reaches
/// it, so it is far above anything a loaded machine needs.
const HANG: Duration = Duration::from_secs(20);

pub type WsSock = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

/// Open the stream with a read timeout, so a frame that never arrives
/// fails the test instead of hanging the suite.
fn ws_open(server: &TestServer) -> WsSock {
    let url = format!("ws://{}/api/stream", server.addr);
    let (socket, _) = tungstenite::connect(&url).expect("ws connect");
    if let tungstenite::stream::MaybeTlsStream::Plain(s) = socket.get_ref() {
        s.set_read_timeout(Some(HANG)).expect("read timeout");
    }
    socket
}

/// Subscribes the socket to the changes `query` picks: their projections,
/// the stored entries past `after` when given, then live entries.
pub fn ws_subscribe(server: &TestServer, query: &Value, after: Option<u64>) -> WsSock {
    let mut socket = ws_open(server);
    let sub = json!({ "query": query, "after": after }).to_string();
    socket
        .send(tungstenite::Message::Text(sub.into()))
        .expect("subscribe");
    socket
}

/// The next frame's `entry` body — a `StreamMessage::Entry`.
pub fn ws_entry(socket: &mut WsSock) -> Value {
    ws_read(socket)["entry"].clone()
}

/// The next Text frame (a `StreamMessage`) parsed as JSON.
///
/// Panics when the socket closes or sends nothing, so a test asserts on
/// the frame it reads and never on the absence of one.
pub fn ws_read(socket: &mut WsSock) -> Value {
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(t)) => {
                return serde_json::from_str(t.as_str()).expect("a stream frame");
            }
            Ok(tungstenite::Message::Ping(p)) => {
                let _ = socket.send(tungstenite::Message::Pong(p));
            }
            Ok(_) => {}
            Err(e) => panic!("the stream sent no frame: {e}"),
        }
    }
}

/// A running `nit` process, so the test can act while a `--wait` waits.
///
/// A reader thread drains each of the child's pipes, so the test wakes on
/// what the child writes and never on a clock.
pub struct RunningNit {
    child: std::process::Child,
    args: String,
    /// The whole stdout, sent once the child closes the pipe.
    stdout: std::sync::mpsc::Receiver<String>,
    /// The whole stderr, sent once the child closes the pipe.
    stderr: std::sync::mpsc::Receiver<String>,
}

/// Spawns `nit` and returns without waiting for it.
pub fn nit_spawn(server: &TestServer, repo: &GitRepo, args: &[&str]) -> RunningNit {
    let mut child = nit_command(server, repo, args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn nit");
    RunningNit {
        stdout: read_to_end(child.stdout.take().expect("stdout")),
        stderr: read_to_end(child.stderr.take().expect("stderr")),
        child,
        args: format!("{args:?}"),
    }
}

/// Reads the pipe to its end and sends the whole text. The send is the
/// child's EOF on that pipe, which is the child exiting.
///
/// One thread per pipe: a child that fills the pipe nobody is reading
/// would block against a reader that takes them in turn.
fn read_to_end(mut pipe: impl std::io::Read + Send + 'static) -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
        let _ = tx.send(text);
    });
    rx
}

impl RunningNit {
    /// Waits for the process to exit and returns its output.
    ///
    /// Panics on a child that never exits, so the suite never hangs.
    pub fn finish(mut self) -> (bool, Value, String) {
        let Ok(stdout) = self.stdout.recv_timeout(HANG) else {
            panic!("nit {} did not finish within {HANG:?}", self.args);
        };
        let status = self.child.wait().expect("wait");
        let stderr = self.stderr.recv().unwrap_or_default();
        (status.success(), parse_stdout(&stdout), stderr)
    }
}

/// Kills the child, so neither a wait a test left running nor its reader
/// threads outlive the test that spawned it.
impl Drop for RunningNit {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// ≡ `nit abandon`, asserting the overlay took.
pub fn abandon(server: &TestServer, change_number: u64) {
    let (st, body) = http_post(
        &server.url(&format!("/api/changes/{change_number}/abandon")),
        &json!({}),
    );
    assert_eq!(st, 200, "abandon change {change_number}: {body}");
    assert_eq!(
        status_at(server, change_number, Some(0)).as_deref(),
        Some("abandoned")
    );
}
