//! `SQLite` persistence layer.
//!
//! Schema contract: `docs/data-model.md` ("Tables"). The database stores
//! only the append-only event `log` (plus `chains` registration identity
//! and reviewer `drafts`); all reviewable state is the fold of the log
//! (`crate::review`), held in memory and rebuilt by replay. Nothing in the
//! log is ever mutated or deleted.
//!
//! [`open`] applies pragmas (WAL, `busy_timeout`, foreign keys ON) and runs
//! `PRAGMA user_version` migrations. Row structs and focused query helpers
//! live here; multi-statement write flows append under a caller-held
//! transaction.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

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
    // v1: the event-log schema — docs/data-model.md "Tables". Earlier
    // pre-1.0 schemas (normalized changes/revisions/comments/reviews/events)
    // are wiped: review state is re-derivable by re-pushing a branch, so the
    // upgrade drops them and starts the log fresh.
    "
    DROP TABLE IF EXISTS events;
    DROP TABLE IF EXISTS comments;
    DROP TABLE IF EXISTS reviews;
    DROP TABLE IF EXISTS revisions;
    DROP TABLE IF EXISTS changes;
    DROP TABLE IF EXISTS chains;
    DROP TABLE IF EXISTS repos;
    CREATE TABLE chains (
      id         INTEGER PRIMARY KEY,
      repo_path  TEXT NOT NULL,
      branch     TEXT NOT NULL,
      base       TEXT NOT NULL,
      created_at TEXT NOT NULL,
      UNIQUE (repo_path, branch)
    );
    CREATE TABLE log (
      chain_id   INTEGER NOT NULL REFERENCES chains(id),
      idx        INTEGER NOT NULL,  -- 0-based, contiguous per chain
      kind       TEXT NOT NULL,
      payload    TEXT NOT NULL DEFAULT '{}',
      created_at TEXT NOT NULL,
      PRIMARY KEY (chain_id, idx)
    );
    CREATE TABLE drafts (
      id               INTEGER PRIMARY KEY,
      chain_id         INTEGER NOT NULL REFERENCES chains(id),
      change_key       TEXT NOT NULL,
      revision         INTEGER NOT NULL,
      parent_id        INTEGER,      -- published comment id (fold-assigned)
      file             TEXT,
      line             INTEGER,
      side             TEXT NOT NULL DEFAULT 'new',
      range_start_line INTEGER,
      range_start_char INTEGER,
      range_end_line   INTEGER,
      range_end_char   INTEGER,
      line_text        TEXT,
      body             TEXT NOT NULL,
      created_at       TEXT NOT NULL,
      updated_at       TEXT NOT NULL
    );
    ",
    // v2: thread resolution is now drafted (docs/api.md "Thread resolution").
    // A draft carries the resolve-checkbox decision it stages on its thread;
    // NULL means no decision. Pre-existing scratch drafts predate the column.
    "ALTER TABLE drafts ADD COLUMN resolved INTEGER;",
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

// ---------------------------------------------------------------------------
// Chains (registration identity only)

#[derive(Debug, Clone)]
pub struct ChainRow {
    pub id: u64,
    pub repo_path: String,
    pub branch: String,
    pub base: String,
    pub created_at: String,
}

fn map_chain(row: &rusqlite::Row) -> rusqlite::Result<ChainRow> {
    Ok(ChainRow {
        id: col_u64(row.get("id")?)?,
        repo_path: row.get("repo_path")?,
        branch: row.get("branch")?,
        base: row.get("base")?,
        created_at: row.get("created_at")?,
    })
}

/// Register or refresh a chain by `(repo_path, branch)`: insert if new,
/// otherwise update `base` (re-registration may change it). Returns the row.
///
/// # Errors
/// On a database failure.
pub fn get_or_create_chain(
    conn: &Connection,
    repo_path: &str,
    branch: &str,
    base: &str,
) -> Result<ChainRow> {
    if let Some(existing) = find_chain(conn, repo_path, branch)? {
        if existing.base != base {
            conn.execute(
                "UPDATE chains SET base = ?1 WHERE id = ?2",
                params![base, i64::try_from(existing.id)?],
            )?;
        }
        return get_chain(conn, existing.id)?
            .ok_or_else(|| anyhow!("chain {} vanished", existing.id));
    }
    conn.execute(
        "INSERT INTO chains (repo_path, branch, base, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![repo_path, branch, base, now_rfc3339()],
    )?;
    let id = col_u64(conn.last_insert_rowid())?;
    get_chain(conn, id)?.ok_or_else(|| anyhow!("chain {id} vanished"))
}

/// # Errors
/// On a database failure.
pub fn find_chain(conn: &Connection, repo_path: &str, branch: &str) -> Result<Option<ChainRow>> {
    conn.query_row(
        "SELECT * FROM chains WHERE repo_path = ?1 AND branch = ?2",
        params![repo_path, branch],
        map_chain,
    )
    .optional()
    .map_err(Into::into)
}

/// # Errors
/// On a database failure.
pub fn get_chain(conn: &Connection, id: u64) -> Result<Option<ChainRow>> {
    conn.query_row(
        "SELECT * FROM chains WHERE id = ?1",
        params![i64::try_from(id)?],
        map_chain,
    )
    .optional()
    .map_err(Into::into)
}

/// All chain rows, id-ascending (registration order).
///
/// # Errors
/// On a database failure.
pub fn all_chains(conn: &Connection) -> Result<Vec<ChainRow>> {
    let mut stmt = conn.prepare("SELECT * FROM chains ORDER BY id")?;
    let rows = stmt
        .query_map([], map_chain)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Log (the append-only event log)

#[derive(Debug, Clone)]
pub struct LogRow {
    pub idx: u64,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

/// `head` = number of entries = idx of the next entry to append.
///
/// # Errors
/// On a database failure.
pub fn log_head(conn: &Connection, chain_id: u64) -> Result<u64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(idx) FROM log WHERE chain_id = ?1",
        params![i64::try_from(chain_id)?],
        |r| r.get(0),
    )?;
    Ok(match max {
        Some(m) => col_u64(m)? + 1,
        None => 0,
    })
}

/// Append one entry at `idx` (must equal the current head; the caller holds
/// the chain lock and computes it).
///
/// # Errors
/// On a database failure (including a primary-key clash on `idx`).
pub fn append_log(
    conn: &Connection,
    chain_id: u64,
    idx: u64,
    kind: &str,
    payload: &Value,
    created_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO log (chain_id, idx, kind, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            i64::try_from(chain_id)?,
            i64::try_from(idx)?,
            kind,
            payload.to_string(),
            created_at
        ],
    )?;
    Ok(())
}

/// Entries in `[from, to)`, idx-ascending. `to = None` means through head.
///
/// # Errors
/// On a database failure.
pub fn log_entries(
    conn: &Connection,
    chain_id: u64,
    from: u64,
    to: Option<u64>,
) -> Result<Vec<LogRow>> {
    let map = |r: &rusqlite::Row| -> rusqlite::Result<LogRow> {
        Ok(LogRow {
            idx: col_u64(r.get("idx")?)?,
            kind: r.get("kind")?,
            payload: r.get("payload")?,
            created_at: r.get("created_at")?,
        })
    };
    let chain_id = i64::try_from(chain_id)?;
    let from = i64::try_from(from)?;
    // `to = None` means "through head": omit the upper bound entirely rather
    // than fake one with a sentinel (an `idx < i64::MAX` clause would drop a
    // hypothetical entry at i64::MAX).
    let rows = match to {
        Some(to) => conn
            .prepare(
                "SELECT idx, kind, payload, created_at FROM log
                 WHERE chain_id = ?1 AND idx >= ?2 AND idx < ?3 ORDER BY idx",
            )?
            .query_map(params![chain_id, from, i64::try_from(to)?], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => conn
            .prepare(
                "SELECT idx, kind, payload, created_at FROM log
                 WHERE chain_id = ?1 AND idx >= ?2 ORDER BY idx",
            )?
            .query_map(params![chain_id, from], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Drafts (reviewer-private scratch; never enters the log)

#[derive(Debug, Clone)]
pub struct DraftRow {
    pub id: u64,
    pub chain_id: u64,
    pub change_key: String,
    pub revision: u64,
    pub parent_id: Option<u64>,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: String,
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
        chain_id: col_u64(row.get("chain_id")?)?,
        change_key: row.get("change_key")?,
        revision: col_u64(row.get("revision")?)?,
        parent_id: col_u64_opt(row.get("parent_id")?)?,
        file: row.get("file")?,
        line: col_u64_opt(row.get("line")?)?,
        side: row.get("side")?,
        range,
        line_text: row.get("line_text")?,
        body: row.get("body")?,
        resolved: row.get::<_, Option<i64>>("resolved")?.map(|v| v != 0),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub struct NewDraft<'a> {
    pub chain_id: u64,
    pub change_key: &'a str,
    pub revision: u64,
    pub parent_id: Option<u64>,
    pub file: Option<&'a str>,
    pub line: Option<u64>,
    pub side: &'a str,
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
    let parent_id = d.parent_id.map(i64::try_from).transpose()?;
    let line = d.line.map(i64::try_from).transpose()?;
    conn.execute(
        "INSERT INTO drafts (id, chain_id, change_key, revision, parent_id, file, line, side,
            range_start_line, range_start_char, range_end_line, range_end_char,
            line_text, body, resolved, created_at, updated_at)
         VALUES (?15, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?16, ?14, ?14)",
        params![
            i64::try_from(d.chain_id)?,
            d.change_key,
            i64::try_from(d.revision)?,
            parent_id,
            d.file,
            line,
            d.side,
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
    let max: Option<i64> = conn.query_row("SELECT MAX(id) FROM drafts", [], |r| r.get(0))?;
    Ok(match max {
        Some(m) => col_u64(m)?,
        None => 0,
    })
}

/// # Errors
/// On a database failure.
pub fn get_draft(conn: &Connection, id: u64) -> Result<Option<DraftRow>> {
    conn.query_row(
        "SELECT * FROM drafts WHERE id = ?1",
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
        "UPDATE drafts SET body = ?1, resolved = ?4, updated_at = ?2 WHERE id = ?3",
        params![body, now, i64::try_from(id)?, resolved.map(i64::from)],
    )?;
    Ok(())
}

/// # Errors
/// On a database failure.
pub fn delete_draft(conn: &Connection, id: u64) -> Result<()> {
    conn.execute(
        "DELETE FROM drafts WHERE id = ?1",
        params![i64::try_from(id)?],
    )?;
    Ok(())
}

/// Drafts for one change, id-ascending.
///
/// # Errors
/// On a database failure.
pub fn drafts_for_change(
    conn: &Connection,
    chain_id: u64,
    change_key: &str,
) -> Result<Vec<DraftRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM drafts WHERE chain_id = ?1 AND change_key = ?2 ORDER BY id")?;
    let rows = stmt
        .query_map(params![i64::try_from(chain_id)?, change_key], map_draft)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Delete every draft of one change (called when its drafts publish).
///
/// # Errors
/// On a database failure.
pub fn delete_drafts_for_change(conn: &Connection, chain_id: u64, change_key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM drafts WHERE chain_id = ?1 AND change_key = ?2",
        params![i64::try_from(chain_id)?, change_key],
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

    #[test]
    fn chain_upsert_is_idempotent_and_updates_base() {
        let conn = mem();
        let a = get_or_create_chain(&conn, "/r", "feat", "main").expect("create");
        // Re-registering with identical args returns the same chain, never a
        // duplicate row.
        let again = get_or_create_chain(&conn, "/r", "feat", "main").expect("re-register");
        assert_eq!(a.id, again.id);
        // A moved base updates the existing row in place.
        let b = get_or_create_chain(&conn, "/r", "feat", "develop").expect("upsert");
        assert_eq!(a.id, b.id);
        assert_eq!(b.base, "develop");
    }

    #[test]
    fn log_append_and_head() {
        let conn = mem();
        let c = get_or_create_chain(&conn, "/r", "feat", "main").expect("create");
        assert_eq!(log_head(&conn, c.id).expect("head"), 0);
        append_log(
            &conn,
            c.id,
            0,
            "partial",
            &serde_json::json!({"partial": true}),
            "t0",
        )
        .expect("append");
        append_log(
            &conn,
            c.id,
            1,
            "reply",
            &serde_json::json!({"replies": []}),
            "t1",
        )
        .expect("append");
        assert_eq!(log_head(&conn, c.id).expect("head"), 2);
        let entries = log_entries(&conn, c.id, 0, None).expect("entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, "partial");
        assert_eq!(entries[1].idx, 1);
        let tail = log_entries(&conn, c.id, 1, None).expect("tail");
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].kind, "reply");
    }

    #[test]
    fn draft_lifecycle() {
        let conn = mem();
        let c = get_or_create_chain(&conn, "/r", "feat", "main").expect("create");
        let d = insert_draft(
            &conn,
            7,
            &NewDraft {
                chain_id: c.id,
                change_key: "I1",
                revision: 1,
                parent_id: None,
                file: Some("src/main.rs"),
                line: Some(3),
                side: "new",
                range: None,
                line_text: Some("fn main"),
                body: "look",
                resolved: None,
            },
            "t0",
        )
        .expect("insert");
        assert_eq!(drafts_for_change(&conn, c.id, "I1").expect("list").len(), 1);
        update_draft(&conn, d.id, "look again", Some(true), "t1").expect("edit");
        let edited = get_draft(&conn, d.id).expect("get").expect("some");
        assert_eq!(edited.body, "look again");
        assert_eq!(edited.resolved, Some(true));
        delete_drafts_for_change(&conn, c.id, "I1").expect("drain");
        assert!(
            drafts_for_change(&conn, c.id, "I1")
                .expect("list")
                .is_empty()
        );
    }
}
