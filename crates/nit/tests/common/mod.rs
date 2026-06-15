//! Shared integration-test harness: a tiny real git repo (built with
//! git2, no worktree needed) plus a temp sqlite db with one registered
//! chain (`feat` onto `main`).
//!
//! Each integration-test binary compiles its own copy, so helpers unused
//! by one binary are fine.
#![expect(
    dead_code,
    reason = "each test binary compiles its own copy and uses a subset"
)]

use std::sync::atomic::{AtomicI64, Ordering};

use git2::{Oid, Repository, RepositoryInitOptions, Signature, Time};
use nit::db::ChainRow;
use nit::enums::LogKind;
use nit::gitscan::{self, ScanResult};
use nit::review::{self, ChainStatus, ChangeProj, Entry, Projection};
use serde_json::{Value, json};

/// Strictly increasing commit timestamps so equal-content commits get
/// distinct shas.
static CLOCK: AtomicI64 = AtomicI64::new(1_700_000_000);

pub fn sig() -> Signature<'static> {
    let t = CLOCK.fetch_add(1, Ordering::SeqCst);
    Signature::new("Test", "test@example.com", &Time::new(t, 0)).unwrap()
}

/// A scan/fold fixture: a real git repo plus the in-memory [`Projection`]
/// the server keeps, driven directly (no HTTP). `scan()` runs the pure
/// `gitscan::scan`, applies its transient state, and folds its entries into
/// the projection — the same sequence the server layer performs.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub repo: Repository,
    pub proj: Projection,
    next_id: u64,
    appended: Vec<String>,
    /// First commit on `main`.
    pub root: Oid,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("refs/heads/main");
        let repo = Repository::init_opts(dir.path().join("repo"), &opts).unwrap();

        let root = commit_in(&repo, &[], "init\n", &[("README", "hello\n")]);
        repo.reference("refs/heads/main", root, true, "test")
            .unwrap();

        let git_dir = git_dir_string(&repo);
        let chain = ChainRow {
            id: 1,
            repo_id: 1,
            git_dir,
            branch: "feat".to_string(),
            base: "main".to_string(),
            created_at: "t0".to_string(),
        };
        Fixture {
            dir,
            repo,
            proj: Projection::empty(&chain),
            next_id: 1000,
            appended: Vec::new(),
            root,
        }
    }

    pub fn commit(&self, parents: &[Oid], message: &str, files: &[(&str, &str)]) -> Oid {
        commit_in(&self.repo, parents, message, files)
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

    /// Run a scan and apply it to the projection; returns the raw result.
    pub fn scan(&mut self) -> ScanResult {
        self.scan_at(jiff::Timestamp::now())
    }

    pub fn scan_at(&mut self, now: jiff::Timestamp) -> ScanResult {
        let mut next = self.next_id;
        let mut alloc = || {
            let id = next;
            next += 1;
            id
        };
        let result = gitscan::scan(&self.proj, now, &mut alloc);
        self.next_id = next;
        self.proj.last_scan_error.clone_from(&result.error);
        self.proj
            .branch_missing_since
            .clone_from(&result.branch_missing_since);
        for e in &result.entries {
            self.fold(e.kind, e.payload.clone());
        }
        result
    }

    /// Live changes (chain order, orphans last).
    pub fn changes(&self) -> Vec<&ChangeProj> {
        self.proj.changes_ordered()
    }

    pub fn change(&self, key: &str) -> &ChangeProj {
        self.proj
            .change_by_key(key)
            .unwrap_or_else(|| panic!("change {key} not found"))
    }

    pub fn status(&self) -> ChainStatus {
        self.proj.status
    }

    pub fn state(&self) -> &'static str {
        review::derive_state(&self.proj)
    }

    pub fn scan_error(&self) -> Option<String> {
        self.proj.last_scan_error.clone()
    }

    /// Count of appended entries of a kind (`revisions`, `chain_closed`, …).
    pub fn appended(&self, kind: &str) -> usize {
        self.appended.iter().filter(|k| *k == kind).count()
    }

    /// Fold a reviewer verdict into the projection (no comments).
    pub fn review(&mut self, key: &str, verdict: &str) {
        let revision = self.change(key).latest_revision().map_or(1, |r| r.number);
        let review_id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "change_key": key, "review_id": review_id, "revision": revision,
            "verdict": verdict, "message": "msg", "comments": [],
        });
        self.fold(LogKind::Review, payload);
    }

    fn fold(&mut self, kind: LogKind, payload: Value) {
        let entry = Entry {
            idx: self.proj.head,
            kind,
            payload,
            created_at: format!("t{}", self.proj.head + 1),
        };
        review::fold(&mut self.proj, &entry).unwrap();
        self.appended.push(kind.as_str().to_string());
    }
}

/// A repo's canonical git-common-dir as a string — the chain's repo identity
/// on the wire (what `nit push` infers from a worktree and sends as `git_dir`).
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
    // An in-memory index handles nested paths (treebuilder is flat-only).
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
// HTTP/API test harness: a bare git repo builder (no db — the server owns
// it), a real `nit::api` server on port 0, and blocking HTTP helpers.

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

    /// The repo's canonical git-common-dir (see [`git_dir_string`]) — the
    /// chain's repo identity on the wire that `nit push` sends as `git_dir`.
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

    /// Bind a specific address: restart "the same server" (same
    /// host:port, same db) after dropping a previous instance.
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
        let listener = rt.block_on(tokio::net::TcpListener::bind(addr));
        let listener = listener.unwrap();
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
            // Graceful like the binary: wait (bounded) for serve_on to
            // finish in-flight responses — dropping/shutting down a tokio
            // runtime *cancels* async tasks at their next yield, it never
            // waits for them.
            if let Some(served) = self.served.take() {
                let _ = rt.block_on(async {
                    tokio::time::timeout(std::time::Duration::from_secs(5), served).await
                });
            }
            rt.shutdown_timeout(std::time::Duration::from_secs(5));
        }
    }
}

/// Run the real `nit` binary (`CARGO_BIN_EXE`) from inside `repo`
/// against `server`: (exit ok, parsed stdout JSON, stderr).
pub fn nit(
    server: &TestServer,
    repo: &GitRepo,
    args: &[&str],
) -> (bool, serde_json::Value, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nit"))
        .args(args)
        .current_dir(repo.workdir())
        .env("NIT_SERVER", &server.base)
        .output()
        .expect("running nit");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let value = serde_json::from_str(stdout.trim()).unwrap_or(serde_json::Value::Null);
    (out.status.success(), value, stderr)
}

/// `nit push` / `nit ready` with the repo path and branch passed
/// explicitly — they have no cwd fallback. `cmd` is `"push"` or `"ready"`;
/// `extra` carries flags like `--partial`.
pub fn nit_register(
    server: &TestServer,
    repo: &GitRepo,
    cmd: &str,
    branch: &str,
    extra: &[&str],
) -> (bool, serde_json::Value, String) {
    let workdir = repo.workdir();
    let workdir = workdir.to_str().expect("workdir path is valid UTF-8");
    let mut args = vec![cmd, "--repo", workdir, "--branch", branch];
    args.extend_from_slice(extra);
    nit(server, repo, &args)
}

fn agent() -> ureq::Agent {
    // Non-2xx responses must come back as (status, body), not errors.
    ureq::config::Config::builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

fn read(mut response: ureq::http::Response<ureq::Body>) -> (u16, serde_json::Value) {
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap();
    let value = if text.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
    };
    (status, value)
}

pub fn http_get(url: &str) -> (u16, serde_json::Value) {
    read(agent().get(url).call().unwrap())
}

/// Connect to an SSE endpoint and collect up to `max` `data:` events,
/// giving up after `idle` with no new bytes (keep-alive comments and other
/// fields are skipped). Each event's data is parsed as JSON.
pub fn sse_collect(url: &str, max: usize, idle: std::time::Duration) -> Vec<serde_json::Value> {
    use std::io::BufRead;
    let agent = ureq::config::Config::builder()
        .http_status_as_error(false)
        .timeout_recv_body(Some(idle))
        .build()
        .new_agent();
    let resp = agent.get(url).call().unwrap();
    let mut reader = std::io::BufReader::new(resp.into_body().into_reader());
    let mut out = Vec::new();
    let (mut data, mut have, mut line) = (String::new(), false, String::new());
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break, // EOF or idle timeout
            Ok(_) => {}
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if have {
                if let Ok(v) = serde_json::from_str(&data) {
                    out.push(v);
                }
                if out.len() >= max {
                    break;
                }
                data.clear();
                have = false;
            }
        } else if let Some(rest) = l.strip_prefix("data:") {
            if have {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            have = true;
        }
    }
    out
}

pub fn http_post(url: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    read(agent().post(url).send_json(body).unwrap())
}

pub fn http_patch(url: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    read(agent().patch(url).send_json(body).unwrap())
}

pub fn http_delete(url: &str) -> (u16, serde_json::Value) {
    read(agent().delete(url).call().unwrap())
}

/// `subject` + `Change-Id` trailer message.
pub fn msg(subject: &str, change_id: &str) -> String {
    format!("{subject}\n\nChange-Id: {change_id}\n")
}
