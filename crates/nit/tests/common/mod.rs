//! Shared integration-test harness: a tiny real git repo (built with
//! git2, no worktree needed) plus a temp sqlite db with one registered
//! chain (`feat` onto `main`).
//!
//! Each integration-test binary compiles its own copy, so helpers unused
//! by one binary are fine.
#![allow(dead_code)]

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

    /// The tree sha of a commit.
    pub fn tree_of(&self, commit: Oid) -> String {
        self.repo.find_commit(commit).unwrap().tree_id().to_string()
    }

    /// Read `path` out of a tree (by sha string).
    pub fn blob_in_tree(&self, tree_sha: &str, path: &str) -> String {
        let tree = self
            .repo
            .find_tree(Oid::from_str(tree_sha).unwrap())
            .unwrap();
        let entry = tree.get_name(path).unwrap();
        let blob = self.repo.find_blob(entry.id()).unwrap();
        String::from_utf8(blob.content().to_vec()).unwrap()
    }
}

fn commit_in(repo: &Repository, parents: &[Oid], message: &str, files: &[(&str, &str)]) -> Oid {
    let parent_commits: Vec<git2::Commit> = parents
        .iter()
        .map(|&oid| repo.find_commit(oid).unwrap())
        .collect();
    let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();
    let base_tree = parent_commits.first().map(|c| c.tree().unwrap());
    let mut builder = repo.treebuilder(base_tree.as_ref()).unwrap();
    for (path, content) in files {
        let blob = repo.blob(content.as_bytes()).unwrap();
        builder.insert(path, blob, 0o100644).unwrap();
    }
    let tree_oid = builder.write().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let s = sig();
    repo.commit(None, &s, &s, message, &tree, &parent_refs)
        .unwrap()
}

/// `subject` + `Change-Id` trailer message.
pub fn msg(subject: &str, change_id: &str) -> String {
    format!("{subject}\n\nChange-Id: {change_id}\n")
}
