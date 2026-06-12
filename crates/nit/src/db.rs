//! `SQLite` persistence layer.
//!
//! Schema contract: `docs/data-model.md` ("Tables"). Review state only —
//! git objects stay in the user's repos. Nothing is ever hard-deleted:
//! rows are status-flagged and every status is re-derivable by a later
//! scan.
//!
//! [`open`] applies pragmas (WAL, `busy_timeout`, foreign keys ON) and runs
//! `PRAGMA user_version` migrations. Row structs and focused query helpers
//! live here; all multi-statement write flows (scans, review submission)
//! are driven by callers inside a single transaction.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// RFC3339 timestamp for "now" (UTC), the format stored in every
/// `created_at`/`updated_at` column.
#[must_use]
pub fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

/// Default database location: `$XDG_DATA_HOME/nit/nit.sqlite3`, falling
/// back to `~/.local/share/nit/nit.sqlite3`.
pub fn default_db_path() -> Result<PathBuf> {
    data_dir(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
    .map(|d| d.join("nit").join("nit.sqlite3"))
}

fn data_dir(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = xdg_data_home
        && p.is_absolute()
    {
        return Ok(p);
    }
    home.map(|h| h.join(".local").join("share"))
        .ok_or_else(|| anyhow!("cannot determine data directory: $HOME is not set"))
}

/// Open (creating if needed) the database at `path`, apply pragmas and
/// run migrations. Parent directories are created.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let conn =
        Connection::open(path).with_context(|| format!("opening database {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

const MIGRATIONS: &[&str] = &[
    // v1: initial schema — docs/data-model.md "Tables".
    "
    CREATE TABLE repos (
      id         INTEGER PRIMARY KEY,
      path       TEXT NOT NULL UNIQUE,
      created_at TEXT NOT NULL
    );
    CREATE TABLE chains (
      id              INTEGER PRIMARY KEY,
      repo_id         INTEGER NOT NULL REFERENCES repos(id),
      branch          TEXT NOT NULL,
      base            TEXT NOT NULL,
      status          TEXT NOT NULL DEFAULT 'active',
      last_scan_error TEXT,
      created_at      TEXT NOT NULL,
      updated_at      TEXT NOT NULL,
      UNIQUE (repo_id, branch)
    );
    CREATE TABLE changes (
      id         INTEGER PRIMARY KEY,
      chain_id   INTEGER NOT NULL REFERENCES chains(id),
      change_key TEXT NOT NULL,
      position   INTEGER,            -- NULL while orphaned
      status     TEXT NOT NULL DEFAULT 'pending',
      UNIQUE (chain_id, change_key)
    );
    CREATE TABLE revisions (
      id             INTEGER PRIMARY KEY,
      change_id      INTEGER NOT NULL REFERENCES changes(id),
      number         INTEGER NOT NULL, -- 1-based patchset number
      commit_sha     TEXT NOT NULL,
      parent_sha     TEXT NOT NULL,
      effective_tree TEXT,             -- NULL = fold conflict
      fixups         TEXT NOT NULL DEFAULT '[]', -- JSON [{sha, message}]
      message        TEXT NOT NULL,
      created_at     TEXT NOT NULL,
      UNIQUE (change_id, number)
    );
    CREATE TABLE reviews (
      id              INTEGER PRIMARY KEY,
      change_id       INTEGER NOT NULL REFERENCES changes(id),
      revision_number INTEGER NOT NULL,
      verdict         TEXT NOT NULL,   -- approve | request_changes | comment
      message         TEXT NOT NULL DEFAULT '',
      created_at      TEXT NOT NULL
    );
    CREATE TABLE comments (
      id              INTEGER PRIMARY KEY,
      change_id       INTEGER NOT NULL REFERENCES changes(id),
      revision_number INTEGER NOT NULL,
      parent_id       INTEGER REFERENCES comments(id),
      author          TEXT NOT NULL,   -- reviewer | agent
      file            TEXT,
      line            INTEGER,
      side            TEXT NOT NULL DEFAULT 'new', -- old | new
      line_text       TEXT,
      body            TEXT NOT NULL,
      state           TEXT NOT NULL DEFAULT 'draft', -- draft | published
      resolved        INTEGER NOT NULL DEFAULT 0,
      review_id       INTEGER REFERENCES reviews(id),
      created_at      TEXT NOT NULL,
      updated_at      TEXT NOT NULL
    );
    CREATE TABLE events (
      id         INTEGER PRIMARY KEY AUTOINCREMENT, -- monotonic cursor
      chain_id   INTEGER NOT NULL REFERENCES chains(id),
      kind       TEXT NOT NULL,
      payload    TEXT NOT NULL DEFAULT '{}',
      created_at TEXT NOT NULL
    );
    ",
];

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = usize::try_from(version).context("PRAGMA user_version is negative")?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        conn.execute_batch(&format!(
            "BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;",
            i + 1
        ))
        .with_context(|| format!("applying migration {}", i + 1))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Enums

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStatus {
    Active,
    Merged,
    Abandoned,
}

impl ChainStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChainStatus::Active => "active",
            ChainStatus::Merged => "merged",
            ChainStatus::Abandoned => "abandoned",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(ChainStatus::Active),
            "merged" => Ok(ChainStatus::Merged),
            "abandoned" => Ok(ChainStatus::Abandoned),
            other => Err(anyhow!("unknown chain status {other:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Pending,
    Approved,
    ChangesRequested,
    Commented,
    Orphaned,
}

impl ChangeStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeStatus::Pending => "pending",
            ChangeStatus::Approved => "approved",
            ChangeStatus::ChangesRequested => "changes_requested",
            ChangeStatus::Commented => "commented",
            ChangeStatus::Orphaned => "orphaned",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(ChangeStatus::Pending),
            "approved" => Ok(ChangeStatus::Approved),
            "changes_requested" => Ok(ChangeStatus::ChangesRequested),
            "commented" => Ok(ChangeStatus::Commented),
            "orphaned" => Ok(ChangeStatus::Orphaned),
            other => Err(anyhow!("unknown change status {other:?}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Row structs

#[derive(Debug, Clone)]
pub struct Repo {
    pub id: i64,
    pub path: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Chain {
    pub id: i64,
    pub repo_id: i64,
    pub branch: String,
    pub base: String,
    pub status: ChainStatus,
    pub last_scan_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub id: i64,
    pub chain_id: i64,
    pub change_key: String,
    pub position: Option<i64>,
    pub status: ChangeStatus,
}

/// One folded `fixup!`/`squash!` commit, stored in `revisions.fixups`
/// (JSON array, branch order).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fixup {
    pub sha: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Revision {
    pub id: i64,
    pub change_id: i64,
    pub number: i64,
    pub commit_sha: String,
    pub parent_sha: String,
    pub effective_tree: Option<String>,
    pub fixups: Vec<Fixup>,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub id: i64,
    pub change_id: i64,
    pub revision_number: i64,
    pub parent_id: Option<i64>,
    pub author: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub side: String,
    pub line_text: Option<String>,
    pub body: String,
    pub state: String,
    pub resolved: bool,
    pub review_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct Review {
    pub id: i64,
    pub change_id: i64,
    pub revision_number: i64,
    pub verdict: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: i64,
    pub chain_id: i64,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Row mapping

fn chain_from_row(row: &rusqlite::Row) -> rusqlite::Result<(Chain, String)> {
    let status: String = row.get("status")?;
    Ok((
        Chain {
            id: row.get("id")?,
            repo_id: row.get("repo_id")?,
            branch: row.get("branch")?,
            base: row.get("base")?,
            status: ChainStatus::Active, // patched below
            last_scan_error: row.get("last_scan_error")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        },
        status,
    ))
}

fn finish_chain((mut chain, status): (Chain, String)) -> Result<Chain> {
    chain.status = ChainStatus::parse(&status)?;
    Ok(chain)
}

fn change_from_row(row: &rusqlite::Row) -> rusqlite::Result<(Change, String)> {
    let status: String = row.get("status")?;
    Ok((
        Change {
            id: row.get("id")?,
            chain_id: row.get("chain_id")?,
            change_key: row.get("change_key")?,
            position: row.get("position")?,
            status: ChangeStatus::Pending, // patched below
        },
        status,
    ))
}

fn finish_change((mut change, status): (Change, String)) -> Result<Change> {
    change.status = ChangeStatus::parse(&status)?;
    Ok(change)
}

fn revision_from_row(row: &rusqlite::Row) -> rusqlite::Result<(Revision, String)> {
    let fixups: String = row.get("fixups")?;
    Ok((
        Revision {
            id: row.get("id")?,
            change_id: row.get("change_id")?,
            number: row.get("number")?,
            commit_sha: row.get("commit_sha")?,
            parent_sha: row.get("parent_sha")?,
            effective_tree: row.get("effective_tree")?,
            fixups: Vec::new(), // patched below
            message: row.get("message")?,
            created_at: row.get("created_at")?,
        },
        fixups,
    ))
}

fn finish_revision((mut rev, fixups): (Revision, String)) -> Result<Revision> {
    rev.fixups = serde_json::from_str(&fixups)
        .with_context(|| format!("revision {}: bad fixups JSON", rev.id))?;
    Ok(rev)
}

fn comment_from_row(row: &rusqlite::Row) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: row.get("id")?,
        change_id: row.get("change_id")?,
        revision_number: row.get("revision_number")?,
        parent_id: row.get("parent_id")?,
        author: row.get("author")?,
        file: row.get("file")?,
        line: row.get("line")?,
        side: row.get("side")?,
        line_text: row.get("line_text")?,
        body: row.get("body")?,
        state: row.get("state")?,
        resolved: row.get::<_, i64>("resolved")? != 0,
        review_id: row.get("review_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn review_from_row(row: &rusqlite::Row) -> rusqlite::Result<Review> {
    Ok(Review {
        id: row.get("id")?,
        change_id: row.get("change_id")?,
        revision_number: row.get("revision_number")?,
        verdict: row.get("verdict")?,
        message: row.get("message")?,
        created_at: row.get("created_at")?,
    })
}

// ---------------------------------------------------------------------------
// Repos & chains

/// Look up or insert the repo row for an (already canonicalized) path.
pub fn get_or_create_repo(conn: &Connection, path: &str) -> Result<Repo> {
    conn.execute(
        "INSERT OR IGNORE INTO repos (path, created_at) VALUES (?1, ?2)",
        params![path, now_rfc3339()],
    )?;
    conn.query_row(
        "SELECT id, path, created_at FROM repos WHERE path = ?1",
        params![path],
        |row| {
            Ok(Repo {
                id: row.get(0)?,
                path: row.get(1)?,
                created_at: row.get(2)?,
            })
        },
    )
    .map_err(Into::into)
}

/// Look up or insert the chain for `(repo, branch)`; `base` is updated on
/// re-registration (idempotent `nit push --base`).
pub fn get_or_create_chain(
    conn: &Connection,
    repo_id: i64,
    branch: &str,
    base: &str,
) -> Result<Chain> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO chains (repo_id, branch, base, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'active', ?4, ?4)
         ON CONFLICT (repo_id, branch) DO UPDATE SET base = excluded.base",
        params![repo_id, branch, base, now],
    )?;
    let row = conn.query_row(
        "SELECT * FROM chains WHERE repo_id = ?1 AND branch = ?2",
        params![repo_id, branch],
        chain_from_row,
    )?;
    finish_chain(row)
}

pub fn get_chain(conn: &Connection, id: i64) -> Result<Option<Chain>> {
    conn.query_row(
        "SELECT * FROM chains WHERE id = ?1",
        params![id],
        chain_from_row,
    )
    .optional()?
    .map(finish_chain)
    .transpose()
}

pub fn list_chains(conn: &Connection) -> Result<Vec<Chain>> {
    let mut stmt = conn.prepare("SELECT * FROM chains ORDER BY id")?;
    let rows = stmt.query_map([], chain_from_row)?;
    rows.map(|r| finish_chain(r?)).collect()
}

/// Repo path for a chain (joined through `repos`).
pub fn chain_repo_path(conn: &Connection, chain_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT repos.path FROM chains JOIN repos ON repos.id = chains.repo_id
         WHERE chains.id = ?1",
        params![chain_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

pub fn chain_set_status(conn: &Connection, id: i64, status: ChainStatus, now: &str) -> Result<()> {
    conn.execute(
        "UPDATE chains SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status.as_str(), now],
    )?;
    Ok(())
}

/// Set or clear `last_scan_error`. `touch` controls whether `updated_at`
/// is bumped (the abandoned-branch two-scan rule keys off the timestamp of
/// the scan that *first* saw the ref missing).
pub fn chain_set_scan_error(
    conn: &Connection,
    id: i64,
    error: Option<&str>,
    now: &str,
    touch: bool,
) -> Result<()> {
    if touch {
        conn.execute(
            "UPDATE chains SET last_scan_error = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, error, now],
        )?;
    } else {
        conn.execute(
            "UPDATE chains SET last_scan_error = ?2 WHERE id = ?1",
            params![id, error],
        )?;
    }
    Ok(())
}

/// Chain row for an (already canonicalized) repo path + branch, if any —
/// without creating anything. CLI clients resolve their chain this way.
pub fn find_chain_by_repo_branch(
    conn: &Connection,
    repo_path: &str,
    branch: &str,
) -> Result<Option<Chain>> {
    conn.query_row(
        "SELECT chains.* FROM chains JOIN repos ON repos.id = chains.repo_id
         WHERE repos.path = ?1 AND chains.branch = ?2",
        params![repo_path, branch],
        chain_from_row,
    )
    .optional()?
    .map(finish_chain)
    .transpose()
}

pub fn chain_touch(conn: &Connection, id: i64, now: &str) -> Result<()> {
    conn.execute(
        "UPDATE chains SET updated_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Changes

pub fn get_change(conn: &Connection, id: i64) -> Result<Option<Change>> {
    conn.query_row(
        "SELECT * FROM changes WHERE id = ?1",
        params![id],
        change_from_row,
    )
    .optional()?
    .map(finish_change)
    .transpose()
}

/// All changes of a chain: live ones first in `position` order, orphaned
/// ones last (by id).
pub fn changes_for_chain(conn: &Connection, chain_id: i64) -> Result<Vec<Change>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM changes WHERE chain_id = ?1
         ORDER BY position IS NULL, position, id",
    )?;
    let rows = stmt.query_map(params![chain_id], change_from_row)?;
    rows.map(|r| finish_change(r?)).collect()
}

pub fn insert_change(
    conn: &Connection,
    chain_id: i64,
    change_key: &str,
    position: i64,
    status: ChangeStatus,
) -> Result<Change> {
    conn.execute(
        "INSERT INTO changes (chain_id, change_key, position, status)
         VALUES (?1, ?2, ?3, ?4)",
        params![chain_id, change_key, position, status.as_str()],
    )?;
    Ok(Change {
        id: conn.last_insert_rowid(),
        chain_id,
        change_key: change_key.to_string(),
        position: Some(position),
        status,
    })
}

pub fn change_key_exists(conn: &Connection, chain_id: i64, change_key: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM changes WHERE chain_id = ?1 AND change_key = ?2",
            params![chain_id, change_key],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub fn change_set_position_status(
    conn: &Connection,
    id: i64,
    position: Option<i64>,
    status: ChangeStatus,
) -> Result<()> {
    conn.execute(
        "UPDATE changes SET position = ?2, status = ?3 WHERE id = ?1",
        params![id, position, status.as_str()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Revisions

pub fn revisions_for_change(conn: &Connection, change_id: i64) -> Result<Vec<Revision>> {
    let mut stmt = conn.prepare("SELECT * FROM revisions WHERE change_id = ?1 ORDER BY number")?;
    let rows = stmt.query_map(params![change_id], revision_from_row)?;
    rows.map(|r| finish_revision(r?)).collect()
}

pub fn get_revision(conn: &Connection, change_id: i64, number: i64) -> Result<Option<Revision>> {
    conn.query_row(
        "SELECT * FROM revisions WHERE change_id = ?1 AND number = ?2",
        params![change_id, number],
        revision_from_row,
    )
    .optional()?
    .map(finish_revision)
    .transpose()
}

pub fn latest_revision(conn: &Connection, change_id: i64) -> Result<Option<Revision>> {
    conn.query_row(
        "SELECT * FROM revisions WHERE change_id = ?1
         ORDER BY number DESC LIMIT 1",
        params![change_id],
        revision_from_row,
    )
    .optional()?
    .map(finish_revision)
    .transpose()
}

#[allow(clippy::too_many_arguments)]
pub fn insert_revision(
    conn: &Connection,
    change_id: i64,
    number: i64,
    commit_sha: &str,
    parent_sha: &str,
    effective_tree: Option<&str>,
    fixups: &[Fixup],
    message: &str,
    now: &str,
) -> Result<Revision> {
    let fixups_json = serde_json::to_string(fixups)?;
    conn.execute(
        "INSERT INTO revisions
           (change_id, number, commit_sha, parent_sha, effective_tree,
            fixups, message, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            change_id,
            number,
            commit_sha,
            parent_sha,
            effective_tree,
            fixups_json,
            message,
            now
        ],
    )?;
    Ok(Revision {
        id: conn.last_insert_rowid(),
        change_id,
        number,
        commit_sha: commit_sha.to_string(),
        parent_sha: parent_sha.to_string(),
        effective_tree: effective_tree.map(str::to_string),
        fixups: fixups.to_vec(),
        message: message.to_string(),
        created_at: now.to_string(),
    })
}

/// Used by the scan's "re-fold if tree missing" repair path.
pub fn revision_set_effective_tree(conn: &Connection, id: i64, tree: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE revisions SET effective_tree = ?2 WHERE id = ?1",
        params![id, tree],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reviews

pub fn insert_review(
    conn: &Connection,
    change_id: i64,
    revision_number: i64,
    verdict: &str,
    message: &str,
    now: &str,
) -> Result<Review> {
    conn.execute(
        "INSERT INTO reviews (change_id, revision_number, verdict, message, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![change_id, revision_number, verdict, message, now],
    )?;
    Ok(Review {
        id: conn.last_insert_rowid(),
        change_id,
        revision_number,
        verdict: verdict.to_string(),
        message: message.to_string(),
        created_at: now.to_string(),
    })
}

/// The most recent review on a given revision of a change, if any. Used to
/// re-derive a change's pre-orphan status (docs/data-model.md: statuses are
/// re-derivable).
pub fn latest_review_on_revision(
    conn: &Connection,
    change_id: i64,
    revision_number: i64,
) -> Result<Option<Review>> {
    conn.query_row(
        "SELECT * FROM reviews WHERE change_id = ?1 AND revision_number = ?2
         ORDER BY id DESC LIMIT 1",
        params![change_id, revision_number],
        review_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn reviews_for_change(conn: &Connection, change_id: i64) -> Result<Vec<Review>> {
    let mut stmt = conn.prepare("SELECT * FROM reviews WHERE change_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![change_id], review_from_row)?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

/// The change's most recent review across all revisions (feedback scope).
pub fn latest_review_for_change(conn: &Connection, change_id: i64) -> Result<Option<Review>> {
    conn.query_row(
        "SELECT * FROM reviews WHERE change_id = ?1 ORDER BY id DESC LIMIT 1",
        params![change_id],
        review_from_row,
    )
    .optional()
    .map_err(Into::into)
}

/// Max revision number carrying a review (`last_reviewed_revision`).
pub fn last_reviewed_revision(conn: &Connection, change_id: i64) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT MAX(revision_number) FROM reviews WHERE change_id = ?1",
        params![change_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Comments

pub fn get_comment(conn: &Connection, id: i64) -> Result<Option<Comment>> {
    conn.query_row(
        "SELECT * FROM comments WHERE id = ?1",
        params![id],
        comment_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn comments_for_change(conn: &Connection, change_id: i64) -> Result<Vec<Comment>> {
    let mut stmt = conn.prepare("SELECT * FROM comments WHERE change_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![change_id], comment_from_row)?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

pub struct NewComment<'a> {
    pub change_id: i64,
    pub revision_number: i64,
    pub parent_id: Option<i64>,
    pub author: &'a str,
    pub file: Option<&'a str>,
    pub line: Option<i64>,
    pub side: &'a str,
    pub line_text: Option<&'a str>,
    pub body: &'a str,
    pub state: &'a str,
    pub resolved: bool,
}

pub fn insert_comment(conn: &Connection, c: &NewComment, now: &str) -> Result<Comment> {
    conn.execute(
        "INSERT INTO comments
           (change_id, revision_number, parent_id, author, file, line, side,
            line_text, body, state, resolved, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        params![
            c.change_id,
            c.revision_number,
            c.parent_id,
            c.author,
            c.file,
            c.line,
            c.side,
            c.line_text,
            c.body,
            c.state,
            i64::from(c.resolved),
            now
        ],
    )?;
    Ok(Comment {
        id: conn.last_insert_rowid(),
        change_id: c.change_id,
        revision_number: c.revision_number,
        parent_id: c.parent_id,
        author: c.author.to_string(),
        file: c.file.map(str::to_string),
        line: c.line,
        side: c.side.to_string(),
        line_text: c.line_text.map(str::to_string),
        body: c.body.to_string(),
        state: c.state.to_string(),
        resolved: c.resolved,
        review_id: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    })
}

pub fn update_draft_body(conn: &Connection, id: i64, body: &str, now: &str) -> Result<()> {
    conn.execute(
        "UPDATE comments SET body = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, body, now],
    )?;
    Ok(())
}

/// Hard-delete a draft (the one deliberate exception to "nothing is ever
/// hard-deleted": unpublished drafts are reviewer-private scratch state,
/// `DELETE /api/drafts/{id}`).
pub fn delete_comment(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM comments WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn comment_set_resolved(conn: &Connection, id: i64, resolved: bool, now: &str) -> Result<()> {
    conn.execute(
        "UPDATE comments SET resolved = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, i64::from(resolved), now],
    )?;
    Ok(())
}

/// Publish every draft on a change under a freshly created review
/// (review submission). Returns the published rows.
pub fn publish_drafts(
    conn: &Connection,
    change_id: i64,
    review_id: i64,
    now: &str,
) -> Result<Vec<Comment>> {
    conn.execute(
        "UPDATE comments SET state = 'published', review_id = ?2, updated_at = ?3
         WHERE change_id = ?1 AND state = 'draft'",
        params![change_id, review_id, now],
    )?;
    let mut stmt =
        conn.prepare("SELECT * FROM comments WHERE change_id = ?1 AND review_id = ?2 ORDER BY id")?;
    let rows = stmt.query_map(params![change_id, review_id], comment_from_row)?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

/// Per-change comment tallies for `ChangeSummary.counts`: published
/// comments (incl. replies), drafts, and unresolved published root threads.
pub fn comment_counts(conn: &Connection, change_id: i64) -> Result<(i64, i64, i64)> {
    conn.query_row(
        "SELECT
           COUNT(*) FILTER (WHERE state = 'published'),
           COUNT(*) FILTER (WHERE state = 'draft'),
           COUNT(*) FILTER (WHERE state = 'published' AND parent_id IS NULL
                              AND resolved = 0)
         FROM comments WHERE change_id = ?1",
        params![change_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Events

pub fn insert_event(
    conn: &Connection,
    chain_id: i64,
    kind: &str,
    payload: &serde_json::Value,
    now: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO events (chain_id, kind, payload, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![chain_id, kind, payload.to_string(), now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Latest event id for a chain (0 when none) — the `/wait` cursor.
pub fn latest_event_id(conn: &Connection, chain_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM events WHERE chain_id = ?1",
        params![chain_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

pub fn events_for_chain(conn: &Connection, chain_id: i64) -> Result<Vec<Event>> {
    let mut stmt = conn.prepare(
        "SELECT id, chain_id, kind, payload, created_at
         FROM events WHERE chain_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![chain_id], |row| {
        Ok(Event {
            id: row.get(0)?,
            chain_id: row.get(1)?,
            kind: row.get(2)?,
            payload: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("nit.sqlite3")).unwrap();
        (dir, conn)
    }

    #[test]
    fn open_applies_pragmas_and_migrations() {
        let (_dir, conn) = temp_db();
        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, i64::try_from(MIGRATIONS.len()).unwrap());
    }

    #[test]
    fn open_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nit.sqlite3");
        open(&path).unwrap();
        let conn = open(&path).unwrap(); // re-running migrations is a no-op
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, i64::try_from(MIGRATIONS.len()).unwrap());
    }

    #[test]
    fn data_dir_resolution() {
        assert_eq!(
            data_dir(Some("/xdg".into()), Some("/home/u".into())).unwrap(),
            PathBuf::from("/xdg")
        );
        // Relative XDG_DATA_HOME is ignored per the basedir spec.
        assert_eq!(
            data_dir(Some("rel".into()), Some("/home/u".into())).unwrap(),
            PathBuf::from("/home/u/.local/share")
        );
        assert_eq!(
            data_dir(None, Some("/home/u".into())).unwrap(),
            PathBuf::from("/home/u/.local/share")
        );
        assert!(data_dir(None, None).is_err());
    }

    #[test]
    fn repo_and_chain_roundtrip() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let again = get_or_create_repo(&conn, "/tmp/r").unwrap();
        assert_eq!(repo.id, again.id);

        let chain = get_or_create_chain(&conn, repo.id, "feat/x", "main").unwrap();
        assert_eq!(chain.status, ChainStatus::Active);
        assert_eq!(chain.last_scan_error, None);

        // Re-registration is idempotent but updates base.
        let chain2 = get_or_create_chain(&conn, repo.id, "feat/x", "develop").unwrap();
        assert_eq!(chain2.id, chain.id);
        assert_eq!(chain2.base, "develop");

        let fetched = get_chain(&conn, chain.id).unwrap().unwrap();
        assert_eq!(fetched.branch, "feat/x");
        assert_eq!(chain_repo_path(&conn, chain.id).unwrap().unwrap(), "/tmp/r");
        assert!(get_chain(&conn, 999).unwrap().is_none());
        assert_eq!(list_chains(&conn).unwrap().len(), 1);
    }

    #[test]
    fn change_and_revision_roundtrip() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "b", "main").unwrap();
        let change = insert_change(&conn, chain.id, "Iabc", 0, ChangeStatus::Pending).unwrap();

        assert!(latest_revision(&conn, change.id).unwrap().is_none());
        let fixups = vec![Fixup {
            sha: "f".repeat(40),
            message: "fixup! subj".into(),
        }];
        let now = now_rfc3339();
        insert_revision(
            &conn,
            change.id,
            1,
            &"a".repeat(40),
            &"b".repeat(40),
            Some(&"c".repeat(40)),
            &fixups,
            "subj\n\nbody",
            &now,
        )
        .unwrap();
        let rev = latest_revision(&conn, change.id).unwrap().unwrap();
        assert_eq!(rev.number, 1);
        assert_eq!(rev.fixups, fixups);
        assert_eq!(rev.effective_tree.as_deref(), Some("c".repeat(40).as_str()));

        // UNIQUE(change_id, number)
        assert!(
            insert_revision(
                &conn,
                change.id,
                1,
                &"a".repeat(40),
                &"b".repeat(40),
                None,
                &[],
                "x",
                &now,
            )
            .is_err()
        );

        // Orphaning: position NULL + status flag, then restore.
        change_set_position_status(&conn, change.id, None, ChangeStatus::Orphaned).unwrap();
        let rows = changes_for_chain(&conn, chain.id).unwrap();
        assert_eq!(rows[0].position, None);
        assert_eq!(rows[0].status, ChangeStatus::Orphaned);
    }

    #[test]
    fn duplicate_change_key_rejected() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "b", "main").unwrap();
        insert_change(&conn, chain.id, "Iabc", 0, ChangeStatus::Pending).unwrap();
        assert!(insert_change(&conn, chain.id, "Iabc", 1, ChangeStatus::Pending).is_err());
    }

    #[test]
    fn live_changes_sort_before_orphans() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "b", "main").unwrap();
        let orphan = insert_change(&conn, chain.id, "I1", 0, ChangeStatus::Pending).unwrap();
        change_set_position_status(&conn, orphan.id, None, ChangeStatus::Orphaned).unwrap();
        insert_change(&conn, chain.id, "I2", 0, ChangeStatus::Pending).unwrap();
        let rows = changes_for_chain(&conn, chain.id).unwrap();
        assert_eq!(rows[0].change_key, "I2");
        assert_eq!(rows[1].change_key, "I1");
    }

    #[test]
    fn events_and_reviews() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "b", "main").unwrap();
        let change = insert_change(&conn, chain.id, "I1", 0, ChangeStatus::Pending).unwrap();
        let now = now_rfc3339();

        let e1 = insert_event(
            &conn,
            chain.id,
            "chain_updated",
            &serde_json::json!({}),
            &now,
        )
        .unwrap();
        let e2 = insert_event(
            &conn,
            chain.id,
            "chain_closed",
            &serde_json::json!({}),
            &now,
        )
        .unwrap();
        assert!(e2 > e1, "event ids are the monotonic cursor");
        assert_eq!(events_for_chain(&conn, chain.id).unwrap().len(), 2);

        insert_review(&conn, change.id, 1, "approve", "lgtm", &now).unwrap();
        insert_review(&conn, change.id, 1, "request_changes", "wait", &now).unwrap();
        let latest = latest_review_on_revision(&conn, change.id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(latest.verdict, "request_changes");
        assert!(
            latest_review_on_revision(&conn, change.id, 2)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn comment_lifecycle_and_counts() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "b", "main").unwrap();
        let change = insert_change(&conn, chain.id, "I1", 0, ChangeStatus::Pending).unwrap();
        let now = now_rfc3339();

        let draft = insert_comment(
            &conn,
            &NewComment {
                change_id: change.id,
                revision_number: 1,
                parent_id: None,
                author: "reviewer",
                file: Some("src/main.rs"),
                line: Some(14),
                side: "new",
                line_text: Some("    let x = 1;"),
                body: "why?",
                state: "draft",
                resolved: false,
            },
            &now,
        )
        .unwrap();
        assert_eq!(draft.state, "draft");
        assert_eq!(comment_counts(&conn, change.id).unwrap(), (0, 1, 0));

        update_draft_body(&conn, draft.id, "why though?", &now).unwrap();
        assert_eq!(
            get_comment(&conn, draft.id).unwrap().unwrap().body,
            "why though?"
        );

        let review = insert_review(&conn, change.id, 1, "request_changes", "m", &now).unwrap();
        let published = publish_drafts(&conn, change.id, review.id, &now).unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].state, "published");
        assert_eq!(published[0].review_id, Some(review.id));
        // published root, unresolved
        assert_eq!(comment_counts(&conn, change.id).unwrap(), (1, 0, 1));

        // Agent reply under the root, then resolve the thread.
        let reply = insert_comment(
            &conn,
            &NewComment {
                change_id: change.id,
                revision_number: 1,
                parent_id: Some(draft.id),
                author: "agent",
                file: Some("src/main.rs"),
                line: Some(14),
                side: "new",
                line_text: None,
                body: "fixed",
                state: "published",
                resolved: false,
            },
            &now,
        )
        .unwrap();
        assert_eq!(reply.parent_id, Some(draft.id));
        comment_set_resolved(&conn, draft.id, true, &now).unwrap();
        assert_eq!(comment_counts(&conn, change.id).unwrap(), (2, 0, 0));
        assert_eq!(comments_for_change(&conn, change.id).unwrap().len(), 2);

        delete_comment(&conn, reply.id).unwrap();
        assert!(get_comment(&conn, reply.id).unwrap().is_none());
    }

    #[test]
    fn review_query_helpers() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "b", "main").unwrap();
        let change = insert_change(&conn, chain.id, "I1", 0, ChangeStatus::Pending).unwrap();
        let now = now_rfc3339();
        assert!(
            latest_review_for_change(&conn, change.id)
                .unwrap()
                .is_none()
        );
        assert_eq!(last_reviewed_revision(&conn, change.id).unwrap(), None);

        insert_review(&conn, change.id, 2, "request_changes", "a", &now).unwrap();
        insert_review(&conn, change.id, 1, "comment", "b", &now).unwrap();
        let latest = latest_review_for_change(&conn, change.id).unwrap().unwrap();
        assert_eq!(latest.verdict, "comment"); // latest by id, not revision
        assert_eq!(last_reviewed_revision(&conn, change.id).unwrap(), Some(2));
        assert_eq!(reviews_for_change(&conn, change.id).unwrap().len(), 2);
    }

    #[test]
    fn chain_lookup_and_event_cursor() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "feat", "main").unwrap();
        assert_eq!(
            find_chain_by_repo_branch(&conn, "/tmp/r", "feat")
                .unwrap()
                .unwrap()
                .id,
            chain.id
        );
        assert!(
            find_chain_by_repo_branch(&conn, "/tmp/r", "other")
                .unwrap()
                .is_none()
        );
        assert!(
            find_chain_by_repo_branch(&conn, "/tmp/x", "feat")
                .unwrap()
                .is_none()
        );

        assert_eq!(latest_event_id(&conn, chain.id).unwrap(), 0);
        let now = now_rfc3339();
        let id = insert_event(
            &conn,
            chain.id,
            "chain_updated",
            &serde_json::json!({}),
            &now,
        )
        .unwrap();
        assert_eq!(latest_event_id(&conn, chain.id).unwrap(), id);
    }

    #[test]
    fn change_and_revision_getters() {
        let (_dir, conn) = temp_db();
        let repo = get_or_create_repo(&conn, "/tmp/r").unwrap();
        let chain = get_or_create_chain(&conn, repo.id, "b", "main").unwrap();
        let change = insert_change(&conn, chain.id, "I1", 0, ChangeStatus::Pending).unwrap();
        assert_eq!(get_change(&conn, change.id).unwrap().unwrap().id, change.id);
        assert!(get_change(&conn, 999).unwrap().is_none());

        let now = now_rfc3339();
        insert_revision(
            &conn,
            change.id,
            1,
            &"a".repeat(40),
            &"b".repeat(40),
            None,
            &[],
            "subj",
            &now,
        )
        .unwrap();
        assert_eq!(
            get_revision(&conn, change.id, 1).unwrap().unwrap().number,
            1
        );
        assert!(get_revision(&conn, change.id, 2).unwrap().is_none());
    }

    #[test]
    fn now_rfc3339_shape() {
        let now = now_rfc3339();
        assert!(jiff::Timestamp::strptime("%FT%T%.f%:z", &now).is_ok() || now.ends_with('Z'));
    }
}
