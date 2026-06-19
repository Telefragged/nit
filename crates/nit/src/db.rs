//! `SQLite` persistence layer.
//!
//! Schema contract: `docs/data-model.md` ("Tables"). Five tables: the
//! `repos` registry, the `changes` identity registry, the append-only event
//! `log` (keyed on the change, with a global `seq`), and the reviewer's
//! `draft_comments` and staged `draft_reviews`. All reviewable state is the
//! fold of the per-change logs (`crate::review`), held in memory and rebuilt
//! by replay. Nothing in the log is ever mutated or deleted.
//!
//! [`open`] applies pragmas (WAL, `busy_timeout`, foreign keys ON) and runs
//! `PRAGMA user_version` migrations. Row structs and focused query helpers
//! live here; multi-statement write flows append under a caller-held
//! transaction.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::enums::{Decision, Side};

/// RFC3339 timestamp for "now" (UTC), the format stored in every
/// `created_at`/`updated_at` column.
#[must_use]
pub fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

/// Default database location: `$XDG_DATA_HOME/nit/nit.sqlite3`, falling
/// back to `~/.local/share/nit/nit.sqlite3`.
///
/// # Errors
/// When neither `$XDG_DATA_HOME` nor `$HOME` is set.
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
///
/// # Errors
/// When the directory or database can't be created or opened, a
/// pragma fails, or a migration fails (including a negative
/// `user_version`).
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
    // v1: the schema — docs/data-model.md "Tables". One `PRAGMA user_version`
    // step per entry; later schema changes append as v2, v3, ….
    "
    CREATE TABLE repos (
      id          INTEGER PRIMARY KEY,
      git_dir     TEXT NOT NULL UNIQUE,   -- canonical git-common-dir; identity + name
      base_branch TEXT NOT NULL           -- the one canonical branch; mergedness tracks it
    );
    CREATE TABLE changes (
      id         INTEGER PRIMARY KEY,      -- rowid; the identity everything carries
      repo_id    INTEGER NOT NULL REFERENCES repos(id),
      change_key TEXT NOT NULL,            -- the Change-Id trailer, verbatim
      created_at TEXT NOT NULL,
      UNIQUE (repo_id, change_key)
    );
    CREATE TABLE log (
      seq        INTEGER PRIMARY KEY AUTOINCREMENT,  -- globally monotone: cross-change order
      change_id  INTEGER NOT NULL REFERENCES changes(id),
      idx        INTEGER NOT NULL,         -- 0-based, contiguous per change
      kind       TEXT NOT NULL,
      payload    TEXT NOT NULL DEFAULT '{}',
      created_at TEXT NOT NULL,
      UNIQUE (change_id, idx)
    );
    CREATE TABLE draft_comments (
      id               INTEGER PRIMARY KEY,
      change_id        INTEGER NOT NULL REFERENCES changes(id),
      revision         INTEGER NOT NULL,
      thread_id        INTEGER,      -- fold-assigned thread id (NULL: new thread)
      file             TEXT,
      line             INTEGER,
      side             TEXT NOT NULL DEFAULT 'new',
      range_start_line INTEGER,
      range_start_char INTEGER,
      range_end_line   INTEGER,
      range_end_char   INTEGER,
      line_text        TEXT,
      body             TEXT NOT NULL,
      resolved         INTEGER,
      created_at       TEXT NOT NULL,
      updated_at       TEXT NOT NULL
    );
    CREATE TABLE draft_reviews (
      change_id INTEGER PRIMARY KEY REFERENCES changes(id),  -- one staged decision per change
      decision  TEXT NOT NULL,   -- a Decision: approve | request_changes | comment | abandon | reopen
      message   TEXT NOT NULL    -- cover note (verdict) or reason (abandon)
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
// Range anchor (shared row + wire shape; docs/api.md "Range comments")

/// Selected-text anchor of a line comment: 1-based lines on the comment's
/// side, 0-based chars, `end_char` exclusive, `end_line` = the comment's
/// `line`. `api::types` re-exports it — the JSON shape is these four
/// fields. These are domain coordinates (always non-negative), so the
/// shape is `u64`; the `SQLite` columns are signed, converted in
/// [`map_draft`]/[`insert_draft`] like every other id (this is the
/// DTO↔domain boundary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommentRange {
    pub start_line: u64,
    pub start_char: u64,
    pub end_line: u64,
    pub end_char: u64,
}

/// Read a column written from a `u64` back as `u64`. Ids, indices and line
/// numbers are stored in `SQLite`'s signed `INTEGER` (its only integer
/// type); a stored negative would mean external corruption, surfaced as an
/// out-of-range error, never a panic. This and [`col_u64_opt`] are the read
/// half of the DTO↔domain boundary — `db.rs` speaks `u64`, `SQLite` `i64`.
fn col_u64(v: i64) -> rusqlite::Result<u64> {
    u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, v))
}

fn col_u64_opt(v: Option<i64>) -> rusqlite::Result<Option<u64>> {
    v.map(col_u64).transpose()
}

/// Parse a stored `side` TEXT column into a [`Side`] (the read half of the
/// db↔domain boundary, like [`col_u64`]). A value that is neither `old` nor
/// `new` means external corruption, surfaced as a conversion error.
fn col_side(s: &str) -> rusqlite::Result<Side> {
    s.parse().map_err(|e: String| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

/// Parse a stored `decision` TEXT column into a [`Decision`] (the read half of
/// the db↔domain boundary, like [`col_side`]). An unknown value means external
/// corruption, surfaced as a conversion error.
fn col_decision(s: &str) -> rusqlite::Result<Decision> {
    s.parse().map_err(|e: String| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

// ---------------------------------------------------------------------------
// Repos (the registry: a canonical git-common-dir → id + its one canonical
// branch — docs/data-model.md "Tables")

#[derive(Debug, Clone)]
pub struct RepoRow {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and its display name.
    pub git_dir: String,
    /// The repo's one canonical branch; mergedness always tracks it.
    pub base_branch: String,
}

fn map_repo(row: &rusqlite::Row) -> rusqlite::Result<RepoRow> {
    Ok(RepoRow {
        id: col_u64(row.get("id")?)?,
        git_dir: row.get("git_dir")?,
        base_branch: row.get("base_branch")?,
    })
}

/// Register or look up a repo by its canonical git-common-dir. Idempotent:
/// the same `git_dir` always maps to the same id. `base_branch` is recorded
/// on the **first** push and never changed here — the row's stored value is
/// returned unchanged on re-registration (the push handler enforces that a
/// later push naming a different base is a 400).
///
/// # Errors
/// On a database failure.
pub fn get_or_create_repo(conn: &Connection, git_dir: &str, base_branch: &str) -> Result<RepoRow> {
    if let Some(existing) = find_repo(conn, git_dir)? {
        return Ok(existing);
    }
    conn.execute(
        "INSERT INTO repos (git_dir, base_branch) VALUES (?1, ?2)",
        params![git_dir, base_branch],
    )?;
    Ok(RepoRow {
        id: col_u64(conn.last_insert_rowid())?,
        git_dir: git_dir.to_string(),
        base_branch: base_branch.to_string(),
    })
}

/// # Errors
/// On a database failure.
pub fn find_repo(conn: &Connection, git_dir: &str) -> Result<Option<RepoRow>> {
    conn.query_row(
        "SELECT * FROM repos WHERE git_dir = ?1",
        params![git_dir],
        map_repo,
    )
    .optional()
    .map_err(Into::into)
}

/// All repos, id-ascending (registration order).
///
/// # Errors
/// On a database failure.
pub fn all_repos(conn: &Connection) -> Result<Vec<RepoRow>> {
    let mut stmt = conn.prepare("SELECT * FROM repos ORDER BY id")?;
    let rows = stmt
        .query_map([], map_repo)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// # Errors
/// On a database failure.
pub fn get_repo(conn: &Connection, id: u64) -> Result<Option<RepoRow>> {
    conn.query_row(
        "SELECT * FROM repos WHERE id = ?1",
        params![i64::try_from(id)?],
        map_repo,
    )
    .optional()
    .map_err(Into::into)
}

/// Repoint a repo at a new canonical git-common-dir (after a disk move). The
/// new `git_dir` must be unique — re-pointing onto another repo's git dir is
/// a `UNIQUE` violation (the caller maps it to a 409).
///
/// # Errors
/// On a database failure, including the `UNIQUE(git_dir)` clash.
pub fn update_repo_git_dir(conn: &Connection, id: u64, git_dir: &str) -> Result<()> {
    conn.execute(
        "UPDATE repos SET git_dir = ?1 WHERE id = ?2",
        params![git_dir, i64::try_from(id)?],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Changes (identity: a (repo, Change-Id) → stable id everything carries)

#[derive(Debug, Clone)]
pub struct ChangeRow {
    pub id: u64,
    pub repo_id: u64,
    pub change_key: String,
    pub created_at: String,
}

fn map_change(row: &rusqlite::Row) -> rusqlite::Result<ChangeRow> {
    Ok(ChangeRow {
        id: col_u64(row.get("id")?)?,
        repo_id: col_u64(row.get("repo_id")?)?,
        change_key: row.get("change_key")?,
        created_at: row.get("created_at")?,
    })
}

/// Upsert a change by `(repo_id, change_key)`, returning its stable id. The
/// `UNIQUE` index makes this idempotent and self-serializing — two pushes
/// first-seeing the same key race one `INSERT … ON CONFLICT DO NOTHING`, the
/// loser falls back to the `SELECT`, and both read the same id.
///
/// # Errors
/// On a database failure.
pub fn upsert_change(conn: &Connection, repo_id: u64, change_key: &str) -> Result<u64> {
    conn.execute(
        "INSERT INTO changes (repo_id, change_key, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (repo_id, change_key) DO NOTHING",
        params![i64::try_from(repo_id)?, change_key, now_rfc3339()],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM changes WHERE repo_id = ?1 AND change_key = ?2",
        params![i64::try_from(repo_id)?, change_key],
        |r| r.get(0),
    )?;
    Ok(col_u64(id)?)
}

/// # Errors
/// On a database failure.
pub fn get_change(conn: &Connection, id: u64) -> Result<Option<ChangeRow>> {
    conn.query_row(
        "SELECT * FROM changes WHERE id = ?1",
        params![i64::try_from(id)?],
        map_change,
    )
    .optional()
    .map_err(Into::into)
}

/// All change rows, id-ascending (creation order) — for replay on startup.
///
/// # Errors
/// On a database failure.
pub fn all_changes(conn: &Connection) -> Result<Vec<ChangeRow>> {
    let mut stmt = conn.prepare("SELECT * FROM changes ORDER BY id")?;
    let rows = stmt
        .query_map([], map_change)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Log (the append-only event log, keyed on the change, globally ordered by seq)

#[derive(Debug, Clone)]
pub struct LogRow {
    /// Globally monotone across the repo — the cross-change order.
    pub seq: u64,
    /// 0-based, contiguous per change.
    pub idx: u64,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

/// `head` = number of entries for a change = idx of its next entry.
///
/// # Errors
/// On a database failure.
pub fn log_head(conn: &Connection, change_id: u64) -> Result<u64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(idx) FROM log WHERE change_id = ?1",
        params![i64::try_from(change_id)?],
        |r| r.get(0),
    )?;
    Ok(match max {
        Some(m) => col_u64(m)? + 1,
        None => 0,
    })
}

/// Append one entry at `idx` (must equal the change's current head; the caller
/// computes it under the change's projection write lock) and return the global
/// `seq` `SQLite` minted for it.
///
/// # Errors
/// On a database failure (including a `UNIQUE(change_id, idx)` clash).
pub fn append_log(
    conn: &Connection,
    change_id: u64,
    idx: u64,
    kind: &str,
    payload: &Value,
    created_at: &str,
) -> Result<u64> {
    conn.execute(
        "INSERT INTO log (change_id, idx, kind, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            i64::try_from(change_id)?,
            i64::try_from(idx)?,
            kind,
            payload.to_string(),
            created_at
        ],
    )?;
    Ok(col_u64(conn.last_insert_rowid())?)
}

fn map_log(row: &rusqlite::Row) -> rusqlite::Result<LogRow> {
    Ok(LogRow {
        seq: col_u64(row.get("seq")?)?,
        idx: col_u64(row.get("idx")?)?,
        kind: row.get("kind")?,
        payload: row.get("payload")?,
        created_at: row.get("created_at")?,
    })
}

/// One change's entries in `[from, to)`, idx-ascending. `to = None` means
/// through head.
///
/// # Errors
/// On a database failure.
pub fn log_entries(
    conn: &Connection,
    change_id: u64,
    from: u64,
    to: Option<u64>,
) -> Result<Vec<LogRow>> {
    let change_id = i64::try_from(change_id)?;
    let from = i64::try_from(from)?;
    // `to = None` means "through head": omit the upper bound entirely rather
    // than fake one with a sentinel.
    let rows = match to {
        Some(to) => conn
            .prepare(
                "SELECT seq, idx, kind, payload, created_at FROM log
                 WHERE change_id = ?1 AND idx >= ?2 AND idx < ?3 ORDER BY idx",
            )?
            .query_map(params![change_id, from, i64::try_from(to)?], map_log)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => conn
            .prepare(
                "SELECT seq, idx, kind, payload, created_at FROM log
                 WHERE change_id = ?1 AND idx >= ?2 ORDER BY idx",
            )?
            .query_map(params![change_id, from], map_log)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Drafts (reviewer-private scratch; never enters the log)

#[derive(Debug, Clone)]
pub struct DraftRow {
    pub id: u64,
    pub change_id: u64,
    pub revision: u64,
    /// The thread this draft replies to; `None` opens a new thread.
    pub thread_id: Option<u64>,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: Side,
    pub range: Option<CommentRange>,
    pub line_text: Option<String>,
    pub body: String,
    /// Staged thread-resolution decision; `None` = none (docs/api.md
    /// "Thread resolution"). Stored as the `resolved` INTEGER column.
    pub resolved: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

fn map_draft(row: &rusqlite::Row) -> rusqlite::Result<DraftRow> {
    let range = match (
        row.get::<_, Option<i64>>("range_start_line")?,
        row.get::<_, Option<i64>>("range_start_char")?,
        row.get::<_, Option<i64>>("range_end_line")?,
        row.get::<_, Option<i64>>("range_end_char")?,
    ) {
        (Some(start_line), Some(start_char), Some(end_line), Some(end_char)) => {
            Some(CommentRange {
                start_line: col_u64(start_line)?,
                start_char: col_u64(start_char)?,
                end_line: col_u64(end_line)?,
                end_char: col_u64(end_char)?,
            })
        }
        _ => None,
    };
    Ok(DraftRow {
        id: col_u64(row.get("id")?)?,
        change_id: col_u64(row.get("change_id")?)?,
        revision: col_u64(row.get("revision")?)?,
        thread_id: col_u64_opt(row.get("thread_id")?)?,
        file: row.get("file")?,
        line: col_u64_opt(row.get("line")?)?,
        side: col_side(&row.get::<_, String>("side")?)?,
        range,
        line_text: row.get("line_text")?,
        body: row.get("body")?,
        resolved: row.get::<_, Option<i64>>("resolved")?.map(|v| v != 0),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub struct NewDraft<'a> {
    pub change_id: u64,
    pub revision: u64,
    pub thread_id: Option<u64>,
    pub file: Option<&'a str>,
    pub line: Option<u64>,
    pub side: Side,
    pub range: Option<CommentRange>,
    pub line_text: Option<&'a str>,
    pub body: &'a str,
    pub resolved: Option<bool>,
}

/// Insert a draft with a caller-allocated `id` (from the server's global
/// fold-id counter, so a draft's id stays stable when it later publishes
/// into a `review` entry — and never collides with any other id).
///
/// # Errors
/// On a database failure.
pub fn insert_draft(conn: &Connection, id: u64, d: &NewDraft, now: &str) -> Result<DraftRow> {
    let (rsl, rsc, rel, rec) = match d.range {
        Some(r) => (
            Some(i64::try_from(r.start_line)?),
            Some(i64::try_from(r.start_char)?),
            Some(i64::try_from(r.end_line)?),
            Some(i64::try_from(r.end_char)?),
        ),
        None => (None, None, None, None),
    };
    let thread_id = d.thread_id.map(i64::try_from).transpose()?;
    let line = d.line.map(i64::try_from).transpose()?;
    conn.execute(
        "INSERT INTO draft_comments (id, change_id, revision, thread_id, file, line, side,
            range_start_line, range_start_char, range_end_line, range_end_char,
            line_text, body, resolved, created_at, updated_at)
         VALUES (?14, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?15, ?13, ?13)",
        params![
            i64::try_from(d.change_id)?,
            i64::try_from(d.revision)?,
            thread_id,
            d.file,
            line,
            d.side.as_str(),
            rsl,
            rsc,
            rel,
            rec,
            d.line_text,
            d.body,
            now,
            i64::try_from(id)?,
            d.resolved.map(i64::from),
        ],
    )?;
    get_draft(conn, id)?.ok_or_else(|| anyhow!("draft {id} vanished"))
}

/// The maximum draft id, for seeding the global id counter on startup.
///
/// # Errors
/// On a database failure.
pub fn max_draft_id(conn: &Connection) -> Result<u64> {
    let max: Option<i64> =
        conn.query_row("SELECT MAX(id) FROM draft_comments", [], |r| r.get(0))?;
    Ok(match max {
        Some(m) => col_u64(m)?,
        None => 0,
    })
}

/// # Errors
/// On a database failure.
pub fn get_draft(conn: &Connection, id: u64) -> Result<Option<DraftRow>> {
    conn.query_row(
        "SELECT * FROM draft_comments WHERE id = ?1",
        params![i64::try_from(id)?],
        map_draft,
    )
    .optional()
    .map_err(Into::into)
}

/// Update a draft's body and its staged resolution decision.
///
/// # Errors
/// On a database failure.
pub fn update_draft(
    conn: &Connection,
    id: u64,
    body: &str,
    resolved: Option<bool>,
    now: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE draft_comments SET body = ?1, resolved = ?4, updated_at = ?2 WHERE id = ?3",
        params![body, now, i64::try_from(id)?, resolved.map(i64::from)],
    )?;
    Ok(())
}

/// # Errors
/// On a database failure.
pub fn delete_draft(conn: &Connection, id: u64) -> Result<()> {
    conn.execute(
        "DELETE FROM draft_comments WHERE id = ?1",
        params![i64::try_from(id)?],
    )?;
    Ok(())
}

/// Drafts for one change, id-ascending.
///
/// # Errors
/// On a database failure.
pub fn drafts_for_change(conn: &Connection, change_id: u64) -> Result<Vec<DraftRow>> {
    let mut stmt = conn.prepare("SELECT * FROM draft_comments WHERE change_id = ?1 ORDER BY id")?;
    let rows = stmt
        .query_map(params![i64::try_from(change_id)?], map_draft)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Delete every draft of one change (called when its drafts publish).
///
/// # Errors
/// On a database failure.
pub fn delete_drafts_for_change(conn: &Connection, change_id: u64) -> Result<()> {
    conn.execute(
        "DELETE FROM draft_comments WHERE change_id = ?1",
        params![i64::try_from(change_id)?],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Draft reviews (the reviewer's staged DECISION; one row per change, never
// in the log — docs/data-model.md "Reviewer decisions")

/// A reviewer's staged decision on a change.
#[derive(Debug, Clone)]
pub struct DraftReviewRow {
    pub change_id: u64,
    pub decision: Decision,
    /// Cover note (for a verdict) or reason (for `abandon`).
    pub message: String,
}

fn map_draft_review(row: &rusqlite::Row) -> rusqlite::Result<DraftReviewRow> {
    Ok(DraftReviewRow {
        change_id: col_u64(row.get("change_id")?)?,
        decision: col_decision(&row.get::<_, String>("decision")?)?,
        message: row.get("message")?,
    })
}

/// Stage (or overwrite) a change's draft decision. One row per change: a later
/// stage replaces the prior decision and message.
///
/// # Errors
/// On a database failure.
pub fn upsert_draft_review(
    conn: &Connection,
    change_id: u64,
    decision: Decision,
    message: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO draft_reviews (change_id, decision, message) VALUES (?1, ?2, ?3)
         ON CONFLICT (change_id) DO UPDATE SET decision = ?2, message = ?3",
        params![i64::try_from(change_id)?, decision.as_str(), message],
    )?;
    Ok(())
}

/// The change's staged decision, if any.
///
/// # Errors
/// On a database failure.
pub fn get_draft_review(conn: &Connection, change_id: u64) -> Result<Option<DraftReviewRow>> {
    conn.query_row(
        "SELECT * FROM draft_reviews WHERE change_id = ?1",
        params![i64::try_from(change_id)?],
        map_draft_review,
    )
    .optional()
    .map_err(Into::into)
}

/// Discard a change's staged decision (called when it publishes, or on an
/// explicit clear). A no-op when nothing is staged.
///
/// # Errors
/// On a database failure.
pub fn delete_draft_review(conn: &Connection, change_id: u64) -> Result<()> {
    conn.execute(
        "DELETE FROM draft_reviews WHERE change_id = ?1",
        params![i64::try_from(change_id)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.pragma_update(None, "foreign_keys", "ON").expect("fk");
        migrate(&conn).expect("migrate");
        conn
    }

    /// A repo + one change on it (the common setup).
    fn change(conn: &Connection) -> u64 {
        let repo = get_or_create_repo(conn, "/r/.git", "main").expect("repo");
        upsert_change(conn, repo.id, "I1").expect("change")
    }

    #[test]
    fn repo_upsert_is_idempotent_and_keeps_base() {
        let conn = mem();
        let a = get_or_create_repo(&conn, "/r/.git", "main").expect("create");
        // Re-registering with a different base does not change the stored one
        // (the push handler rejects a base mismatch; the row is canonical).
        let again = get_or_create_repo(&conn, "/r/.git", "develop").expect("re-register");
        assert_eq!(a.id, again.id);
        assert_eq!(again.base_branch, "main");
        // A different git dir is a distinct repo.
        let b = get_or_create_repo(&conn, "/other/.git", "main").expect("create");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn change_upsert_is_idempotent() {
        let conn = mem();
        let repo = get_or_create_repo(&conn, "/r/.git", "main").expect("repo");
        let a = upsert_change(&conn, repo.id, "Iabc").expect("create");
        let again = upsert_change(&conn, repo.id, "Iabc").expect("re-upsert");
        assert_eq!(a, again);
        // A different key is a distinct change.
        let b = upsert_change(&conn, repo.id, "Idef").expect("create");
        assert_ne!(a, b);
        assert_eq!(
            get_change(&conn, a).expect("get").expect("some").change_key,
            "Iabc"
        );
    }

    #[test]
    fn log_append_mints_seq_and_idx() {
        let conn = mem();
        let c = change(&conn);
        assert_eq!(log_head(&conn, c).expect("head"), 0);
        let s0 = append_log(
            &conn,
            c,
            0,
            "partial",
            &serde_json::json!({"partial": true}),
            "t0",
        )
        .expect("append");
        let s1 = append_log(
            &conn,
            c,
            1,
            "comment",
            &serde_json::json!({"body": "note"}),
            "t1",
        )
        .expect("append");
        assert!(s1 > s0, "seq is monotone");
        assert_eq!(log_head(&conn, c).expect("head"), 2);
        let entries = log_entries(&conn, c, 0, None).expect("entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, "partial");
        assert_eq!(entries[1].idx, 1);
        let tail = log_entries(&conn, c, 1, None).expect("tail");
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].kind, "comment");
    }

    #[test]
    fn seq_is_global_across_changes() {
        let conn = mem();
        let repo = get_or_create_repo(&conn, "/r/.git", "main").expect("repo");
        let a = upsert_change(&conn, repo.id, "Ia").expect("a");
        let b = upsert_change(&conn, repo.id, "Ib").expect("b");
        let sa = append_log(&conn, a, 0, "comment", &serde_json::json!({}), "t0").expect("a0");
        let sb = append_log(&conn, b, 0, "comment", &serde_json::json!({}), "t1").expect("b0");
        let sa1 = append_log(&conn, a, 1, "comment", &serde_json::json!({}), "t2").expect("a1");
        // Both changes' idx restart at 0, but seq totally orders the interleave.
        assert!(sa < sb && sb < sa1);
    }

    #[test]
    fn draft_lifecycle() {
        let conn = mem();
        let c = change(&conn);
        let d = insert_draft(
            &conn,
            7,
            &NewDraft {
                change_id: c,
                revision: 1,
                thread_id: None,
                file: Some("src/main.rs"),
                line: Some(3),
                side: Side::New,
                range: None,
                line_text: Some("fn main"),
                body: "look",
                resolved: None,
            },
            "t0",
        )
        .expect("insert");
        assert_eq!(drafts_for_change(&conn, c).expect("list").len(), 1);
        update_draft(&conn, d.id, "look again", Some(true), "t1").expect("edit");
        let edited = get_draft(&conn, d.id).expect("get").expect("some");
        assert_eq!(edited.body, "look again");
        assert_eq!(edited.resolved, Some(true));
        delete_drafts_for_change(&conn, c).expect("drain");
        assert!(drafts_for_change(&conn, c).expect("list").is_empty());
    }

    #[test]
    fn draft_review_upsert_get_delete() {
        let conn = mem();
        let c = change(&conn);
        assert!(get_draft_review(&conn, c).expect("get").is_none());

        upsert_draft_review(&conn, c, Decision::RequestChanges, "fix this").expect("stage");
        let staged = get_draft_review(&conn, c).expect("get").expect("some");
        assert_eq!(staged.decision, Decision::RequestChanges);
        assert_eq!(staged.message, "fix this");

        // A second stage overwrites (one row per change).
        upsert_draft_review(&conn, c, Decision::Approve, "lgtm").expect("restage");
        let staged = get_draft_review(&conn, c).expect("get").expect("some");
        assert_eq!(staged.decision, Decision::Approve);
        assert_eq!(staged.message, "lgtm");

        delete_draft_review(&conn, c).expect("clear");
        assert!(get_draft_review(&conn, c).expect("get").is_none());
        // Deleting again is a no-op.
        delete_draft_review(&conn, c).expect("clear again");
    }
}
