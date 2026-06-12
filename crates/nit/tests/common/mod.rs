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
use nit::db::{self, ChangeStatus};
use nit::gitscan::{self, ScanOutcome};
use rusqlite::Connection;

/// Strictly increasing commit timestamps so equal-content commits get
/// distinct shas.
static CLOCK: AtomicI64 = AtomicI64::new(1_700_000_000);

pub fn sig() -> Signature<'static> {
    let t = CLOCK.fetch_add(1, Ordering::SeqCst);
    Signature::new("Test", "test@example.com", &Time::new(t, 0)).unwrap()
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub repo: Repository,
    pub conn: Connection,
    pub chain_id: i64,
    /// First commit on `main`.
    pub root: Oid,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("refs/heads/main");
        let repo = Repository::init_opts(dir.path().join("repo"), &opts).unwrap();
        let conn = db::open(&dir.path().join("nit.sqlite3")).unwrap();

        let root = commit_in(&repo, &[], "init\n", &[("README", "hello\n")]);
        repo.reference("refs/heads/main", root, true, "test")
            .unwrap();

        let repo_row =
            db::get_or_create_repo(&conn, repo.path().parent().unwrap().to_str().unwrap()).unwrap();
        let chain = db::get_or_create_chain(&conn, repo_row.id, "feat", "main").unwrap();
        Fixture {
            dir,
            repo,
            conn,
            chain_id: chain.id,
            root,
        }
    }

    /// Create a commit (object only; point a branch at it with
    /// [`Fixture::branch`]). `files` upserts paths at the repo root onto
    /// the first parent's tree.
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

    pub fn scan(&mut self) -> ScanOutcome {
        gitscan::scan(&mut self.conn, self.chain_id).unwrap()
    }

    pub fn scan_at(&mut self, now: jiff::Timestamp) -> ScanOutcome {
        gitscan::scan_at(&mut self.conn, self.chain_id, now).unwrap()
    }

    pub fn changes(&self) -> Vec<db::Change> {
        db::changes_for_chain(&self.conn, self.chain_id).unwrap()
    }

    pub fn latest_rev(&self, change_id: i64) -> db::Revision {
        db::latest_revision(&self.conn, change_id).unwrap().unwrap()
    }

    pub fn chain(&self) -> db::Chain {
        db::get_chain(&self.conn, self.chain_id).unwrap().unwrap()
    }

    pub fn events(&self, kind: &str) -> usize {
        db::events_for_chain(&self.conn, self.chain_id)
            .unwrap()
            .iter()
            .filter(|e| e.kind == kind)
            .count()
    }

    /// Simulate a review submission the way the server layer will: review
    /// row on the latest revision + change status flip.
    pub fn review(&self, change_id: i64, verdict: &str) {
        let rev = self.latest_rev(change_id);
        let now = db::now_rfc3339();
        db::insert_review(&self.conn, change_id, rev.number, verdict, "msg", &now).unwrap();
        let status = match verdict {
            "approve" => ChangeStatus::Approved,
            "request_changes" => ChangeStatus::ChangesRequested,
            _ => ChangeStatus::Commented,
        };
        let row = self
            .changes()
            .into_iter()
            .find(|c| c.id == change_id)
            .unwrap();
        db::change_set_position_status(&self.conn, change_id, row.position, status).unwrap();
    }

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
