//! `SQLite` persistence layer.
//!
//! This module's docs are the schema contract. Six tables: the `repos`
//! registry, the `changes` identity registry and its denormalized
//! `change_tags`, the append-only event `log` (keyed on the change, with a
//! global `sequence`), and the reviewer's `draft_comments` and
//! `draft_reviews`. All reviewable state is the fold of the per-change logs
//! (`crate::review`), held in memory and rebuilt by replay. Nothing in the
//! log is ever mutated or deleted.
//!
//! [`pool`] hands out connections with the pragmas applied (WAL,
//! `busy_timeout`, foreign keys ON); `PRAGMA user_version` migrations run
//! once at startup. Row structs and focused query helpers live here.
//!
//! A read takes a `&Connection` and a write takes a `&Transaction`. The
//! signature is the contract, so no caller reaches a mutating helper here
//! outside a transaction, and [`write()`] is the only place one opens.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use deadpool_sqlite::{Config, Hook, HookError, Pool, Runtime};
use nit_types::domain::ChangeNumber;
use nit_types::domain::{Anchor, CommentRange, LineAnchor};
use nit_types::domain::{ChangeId, ChangeStatus, Decision, RevisionNumber, Sha};
use nit_types::domain::{Tag, Tags};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

/// RFC3339 timestamp for "now" (UTC).
///
/// The format stored in every `created_at`/`updated_at` column.
#[must_use]
pub fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

/// Default database location: `$XDG_DATA_HOME/nit/nit.sqlite3`.
///
/// Falls back to `~/.local/share/nit/nit.sqlite3`.
///
/// # Errors
///
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

/// A connection pool for the database at `path`.
///
/// Creates parent directories. Every pooled connection is prepared with
/// `prepare` (WAL, busy timeout, foreign keys) by a post-create hook; the
/// schema is migrated once at startup by `migrate`, run on the first
/// pooled connection in `AppState::load`.
///
/// # Errors
///
/// When the parent directory can't be created.
pub fn pool(path: &Path) -> Result<Pool> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let pool = Config::new(path)
        .builder(Runtime::Tokio1)?
        .post_create(Hook::async_fn(|conn, _| {
            Box::pin(async move {
                conn.interact(prepare)
                    .await
                    .map_err(|e| HookError::message(e.to_string()))?
                    .map_err(HookError::Backend)
            })
        }))
        .build()?;
    Ok(pool)
}

/// Per-connection setup applied to every pooled connection.
///
/// WAL journaling (a persistent, idempotent database property), a
/// 5-second busy timeout, and foreign keys ON.
fn prepare(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Runs `f` inside a transaction, committing when it succeeds.
///
/// An error rolls the whole thing back, so a write can never land
/// half-applied. The transaction is `IMMEDIATE`: it takes the write lock
/// up front, which `busy_timeout` waits out, where a deferred one that
/// read before writing would fail the upgrade with `SQLITE_BUSY_SNAPSHOT`
/// — an error no timeout retries.
///
/// # Errors
///
/// When the transaction cannot be opened, `f` fails, or the commit fails.
pub fn write<T>(conn: &mut Connection, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

const MIGRATIONS: &[&str] = &[
    // v1: the schema. One `PRAGMA user_version` step per entry; later
    // schema changes append as v2, v3, ….
    "
    CREATE TABLE repos (
      id          INTEGER PRIMARY KEY,
      git_dir     TEXT NOT NULL UNIQUE,   -- canonical git-common-dir; identity + name
      base_branch TEXT NOT NULL           -- the one canonical ref; mergedness tracks it
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
      idx        INTEGER NOT NULL,         -- 0-based per change (MAX(idx)+1 to append)
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
      change_id INTEGER PRIMARY KEY REFERENCES changes(id),  -- one draft decision per change
      decision  TEXT NOT NULL,   -- a Decision: approve | request_changes | comment | abandon | reopen
      message   TEXT NOT NULL    -- cover note (verdict) or reason (abandon)
    );
    ",
    // v2: the merge timer's baseline — the canonical-branch HEAD it last
    // reconciled against, so each sweep scans only the new commits and resumes
    // across restarts. NULL until the first observation; set at
    // `nit repo create` to the branch's then-HEAD.
    "ALTER TABLE repos ADD COLUMN base_head TEXT;",
    // v3: a denormalized cache of each change's current status — the displayed
    // status at its latest revision (`review::ChangeProjection::current_status`). The
    // fold of the change's log stays authoritative; this column exists so a
    // query can filter/scan changes by status without replaying every log.
    // The insert writes this and every append re-stamps it in its own
    // transaction, so nothing reconciles it at run time.
    "ALTER TABLE changes ADD COLUMN status TEXT;",
    // v4: `base_ref` stores a git ref, not necessarily a local branch.
    "ALTER TABLE repos RENAME COLUMN base_branch TO base_ref;",
    // v5: the `partial` log kind's flag never affected a change's stored
    // status (it only gated the read-derived `approved` chain state), so
    // its entries are inert — drop them so replay no longer meets an
    // unknown kind. Revision payloads keep a now-dead `partial` field,
    // ignored by omission: `RevisionPayload` is not `deny_unknown_fields`,
    // so serde drops the unrecognized key on deserialize. Deleting
    // interior entries leaves idx gaps, harmless: the next idx is
    // `MAX(idx) + 1` and the fold orders by idx, never assuming
    // contiguity.
    "DELETE FROM log WHERE kind = 'partial';",
    // v6: the tracked ref is the repo's canonical ref, and the timer's
    // baseline is that ref's head — one name for the thing a chain forks
    // from and mergedness is decided against.
    "ALTER TABLE repos RENAME COLUMN base_ref TO canonical_ref;
     ALTER TABLE repos RENAME COLUMN base_head TO canonical_head;",
    // v7: a revision records the commit it forked from, so the key naming
    // it is `fork_sha`. Revision payloads are serialized `RevisionPayload`,
    // and the key rename has to reach the rows already written or their
    // fork point deserializes as empty.
    "UPDATE log
        SET payload = json_remove(
              json_set(payload, '$.fork_sha', json_extract(payload, '$.base_sha')),
              '$.base_sha')
      WHERE kind = 'revision' AND json_extract(payload, '$.base_sha') IS NOT NULL;",
    // v8: the log's two orderings spell themselves out — `position` for an
    // entry's 0-based place in its change, `sequence` for the global order.
    // `index` would have been the literal expansion of `idx`, but SQLite
    // reserves it, and `position` is already the wire's word for a 0-based
    // ordinal.
    "ALTER TABLE log RENAME COLUMN idx TO position;
     ALTER TABLE log RENAME COLUMN seq TO sequence;",
    // v9: `change_id` is the `Change-Id` trailer the commit carries, so the
    // column holding it takes that name; the rowid every other table joins
    // on is the change's number.
    "ALTER TABLE changes RENAME COLUMN change_key TO change_id;
     ALTER TABLE log RENAME COLUMN change_id TO change_number;
     ALTER TABLE draft_comments RENAME COLUMN change_id TO change_number;
     ALTER TABLE draft_reviews RENAME COLUMN change_id TO change_number;",
    // v10: a denormalized cache of each change's tags, the effective set
    // at its latest revision (`ChangeProjection::tags`). The fold of the
    // change's log stays authoritative. These rows exist so a query can
    // select changes by tag cold, without resolving or replaying one. One
    // value per key matches the fold's overwrite semantics. Any append that
    // moves the set rewrites them in its own transaction, and nothing
    // reconciles them at run time.
    "
    CREATE TABLE change_tags (
      change_number INTEGER NOT NULL REFERENCES changes(id),
      key           TEXT NOT NULL,
      value         TEXT NOT NULL,
      PRIMARY KEY (change_number, key)
    ) WITHOUT ROWID;
    -- The lookup a `?tag=key=value` filter drives; WITHOUT ROWID appends the
    -- primary key, so this index carries change_number and covers the select.
    CREATE INDEX change_tags_by_value ON change_tags (key, value);
    ",
    // v11: the last repair of the status cache. The insert writes the
    // status and every append re-stamps it, so from here only an unmigrated
    // database holds a row behind the fold. A change with no append folds to
    // `pending`, and an earlier startup pass reconciled every row that has
    // one.
    "UPDATE changes SET status = 'pending' WHERE status IS NULL;",
];

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    migrate_to(conn, MIGRATIONS.len())
}

/// Applies migrations up to `upto`, the `PRAGMA user_version` to reach.
///
/// A test stops short to write rows the way an older schema spelled
/// them, and then migrates over them.
fn migrate_to(conn: &mut Connection, upto: usize) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = usize::try_from(version).context("PRAGMA user_version is negative")?;
    for (i, sql) in MIGRATIONS.iter().enumerate().take(upto).skip(version) {
        write(conn, |tx| {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i64::try_from(i + 1)?)?;
            Ok(())
        })
        .with_context(|| format!("applying migration {}", i + 1))?;
    }
    Ok(())
}

/// Reads a column written from a `u64` back as `u64`.
///
/// Ids, indices and line numbers are stored in `SQLite`'s signed
/// `INTEGER` (its only integer type); a stored negative would mean
/// external corruption, surfaced as an out-of-range error, never a panic.
/// This and [`col_u64_opt`] are the read half of the DTO↔domain boundary
/// — `db.rs` speaks `u64`, `SQLite` `i64`.
fn col_u64(v: i64) -> rusqlite::Result<u64> {
    u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, v))
}

/// A value parsed on the way out of the column that held it.
///
/// A stored value that no longer parses means external corruption,
/// surfaced as a conversion error, never a panic.
fn stored<T, E>(parsed: Result<T, E>, column: rusqlite::types::Type) -> rusqlite::Result<T>
where
    E: std::error::Error + Send + Sync + 'static,
{
    parsed.map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, column, Box::new(e)))
}

fn col_u64_opt(v: Option<i64>) -> rusqlite::Result<Option<u64>> {
    v.map(col_u64).transpose()
}

/// Parses a stored TEXT column into a closed-vocab enum.
///
/// The read half of the db↔domain boundary, like [`col_u64`]. An unknown
/// value means external corruption, surfaced as a conversion error.
fn col_enum<T: std::str::FromStr>(s: &str) -> rusqlite::Result<T>
where
    T::Err: std::fmt::Display,
{
    s.parse().map_err(|e: T::Err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            e.to_string().into(),
        )
    })
}

/// Parses a stored `status` TEXT column into a [`ChangeStatus`].
///
/// The read half of the db↔domain boundary, like [`col_decision`]. An
/// unknown value means external corruption, surfaced as a conversion
/// error.
fn col_change_status(s: &str) -> rusqlite::Result<ChangeStatus> {
    s.parse().map_err(|e: String| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

// ---------------------------------------------------------------------------
// Repos (the registry: a canonical git-common-dir → id + its one canonical
// branch)

#[derive(Debug, Clone)]
pub struct RepoRow {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and its display name.
    pub git_dir: String,
    /// The repo's one canonical ref; mergedness always tracks it.
    pub canonical_ref: String,
    /// The canonical-branch HEAD the merge timer last reconciled against.
    ///
    /// `None` until first observed.
    pub canonical_head: Option<Sha>,
}

fn map_repo(row: &rusqlite::Row) -> rusqlite::Result<RepoRow> {
    Ok(RepoRow {
        id: col_u64(row.get("id")?)?,
        git_dir: row.get("git_dir")?,
        canonical_ref: row.get("canonical_ref")?,
        canonical_head: row
            .get::<_, Option<String>>("canonical_head")?
            .map(|v| stored(Sha::new(v), rusqlite::types::Type::Text))
            .transpose()?,
    })
}

/// Registers a fresh repo by its canonical git-common-dir.
///
/// Returns the new row. The caller has already rejected an existing
/// `git_dir` (409); the `UNIQUE(git_dir)` index is the backstop on a
/// race.
///
/// # Errors
///
/// On a database failure, including the `UNIQUE(git_dir)` clash.
pub fn create_repo(tx: &Transaction, git_dir: &str, canonical_ref: &str) -> Result<RepoRow> {
    tx.execute(
        "INSERT INTO repos (git_dir, canonical_ref) VALUES (?1, ?2)",
        params![git_dir, canonical_ref],
    )?;
    Ok(RepoRow {
        id: col_u64(tx.last_insert_rowid())?,
        git_dir: git_dir.to_string(),
        canonical_ref: canonical_ref.to_string(),
        canonical_head: None,
    })
}

/// # Errors
///
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
///
/// On a database failure.
pub fn all_repos(conn: &Connection) -> Result<Vec<RepoRow>> {
    let mut stmt = conn.prepare("SELECT * FROM repos ORDER BY id")?;
    let rows = stmt
        .query_map([], map_repo)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// # Errors
///
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

/// Repoints a repo at a new canonical git-common-dir.
///
/// Used after a disk move. The new `git_dir` must be unique — re-pointing
/// onto another repo's git dir is a `UNIQUE` violation (the caller maps
/// it to a 409).
///
/// # Errors
///
/// On a database failure, including the `UNIQUE(git_dir)` clash.
pub fn update_repo_git_dir(tx: &Transaction, id: u64, git_dir: &str) -> Result<()> {
    tx.execute(
        "UPDATE repos SET git_dir = ?1 WHERE id = ?2",
        params![git_dir, i64::try_from(id)?],
    )?;
    Ok(())
}

/// Records the canonical-branch HEAD the merge timer reconciled against.
///
/// The next sweep then scans only newer commits.
///
/// # Errors
///
/// On a database failure.
pub fn update_repo_canonical_head(tx: &Transaction, id: u64, canonical_head: &Sha) -> Result<()> {
    tx.execute(
        "UPDATE repos SET canonical_head = ?1 WHERE id = ?2",
        params![canonical_head.as_str(), i64::try_from(id)?],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Changes (identity: a (repo, Change-Id) → the number everything carries)

#[derive(Debug, Clone)]
pub struct ChangeRow {
    pub id: ChangeNumber,
    pub repo_id: u64,
    pub change_id: ChangeId,
    /// The denormalized status cache; authoritative state is the fold.
    ///
    /// `None` only for a row that an older binary wrote, before the
    /// insert filled the column.
    pub status: Option<ChangeStatus>,
    pub created_at: String,
}

fn map_change(row: &rusqlite::Row) -> rusqlite::Result<ChangeRow> {
    Ok(ChangeRow {
        id: ChangeNumber::new(col_u64(row.get("id")?)?),
        repo_id: col_u64(row.get("repo_id")?)?,
        change_id: stored(
            ChangeId::new(row.get::<_, String>("change_id")?),
            rusqlite::types::Type::Text,
        )?,
        status: row
            .get::<_, Option<String>>("status")?
            .map(|s| col_change_status(&s))
            .transpose()?,
        created_at: row.get("created_at")?,
    })
}

/// Upserts a change by `(repo_id, change_id)`.
///
/// Returns its number. The `UNIQUE` index makes this idempotent and
/// self-serializing — two pushes first-seeing the same trailer race one
/// `INSERT … ON CONFLICT DO NOTHING`, the loser falls back to the
/// `SELECT`, and both read the same id.
///
/// # Errors
///
/// On a database failure.
pub fn upsert_change(tx: &Transaction, repo_id: u64, change_id: &ChangeId) -> Result<ChangeNumber> {
    tx.execute(
        "INSERT INTO changes (repo_id, change_id, created_at, status)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (repo_id, change_id) DO NOTHING",
        params![
            i64::try_from(repo_id)?,
            change_id.as_str(),
            now_rfc3339(),
            ChangeStatus::Pending.as_str()
        ],
    )?;
    let id: i64 = tx.query_row(
        "SELECT id FROM changes WHERE repo_id = ?1 AND change_id = ?2",
        params![i64::try_from(repo_id)?, change_id.as_str()],
        |r| r.get(0),
    )?;
    Ok(ChangeNumber::new(col_u64(id)?))
}

/// Returns the number a `Change-Id` names in one repo, if any.
///
/// The point-read behind history enrichment (`GET /api/history`).
///
/// # Errors
///
/// On a database failure.
pub fn change_number_by_id(
    conn: &Connection,
    repo_id: u64,
    change_id: &ChangeId,
) -> Result<Option<ChangeNumber>> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM changes WHERE repo_id = ?1 AND change_id = ?2",
            params![i64::try_from(repo_id)?, change_id.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(col_u64_opt(id)?.map(ChangeNumber::new))
}

/// Re-stamps a change's denormalized `status`.
///
/// The fold's current status, cached so a query need not replay the log.
/// The change's log stays the source of truth; this is called inside the
/// same transaction as the append that moved the fold, and on startup to
/// backfill from replay.
///
/// # Errors
///
/// On a database failure.
pub fn update_change_status(
    tx: &Transaction,
    id: ChangeNumber,
    status: ChangeStatus,
) -> Result<()> {
    tx.execute(
        "UPDATE changes SET status = ?1 WHERE id = ?2",
        params![status.as_str(), i64::try_from(id.get())?],
    )?;
    Ok(())
}

/// # Errors
///
/// On a database failure.
pub fn get_change(conn: &Connection, id: ChangeNumber) -> Result<Option<ChangeRow>> {
    conn.query_row(
        "SELECT * FROM changes WHERE id = ?1",
        params![i64::try_from(id.get())?],
        map_change,
    )
    .optional()
    .map_err(Into::into)
}

/// Which of a repo's changes a list read admits.
///
/// Every field narrows and an empty field does not, so the default
/// admits the whole repo.
#[derive(Debug, Default, Clone)]
pub struct ChangeFilter {
    /// Matched against the change's status at its latest revision.
    pub statuses: Vec<ChangeStatus>,
    /// Each must be present, verbatim key and value.
    pub tags: Vec<Tag>,
}

/// One repo's change rows, ascending by number (creation order).
///
/// A repo view derives its chains over this enumeration. The
/// denormalized `status` column and `change_tags` answer it, so nothing
/// resolves or replays a change that the filter excludes. Whole rows
/// rather than numbers, so resolving one needs no second read.
///
/// # Errors
///
/// On a database failure.
pub fn repo_changes(
    conn: &Connection,
    repo_id: u64,
    filter: &ChangeFilter,
) -> Result<Vec<ChangeRow>> {
    let mut sql = String::from("SELECT * FROM changes WHERE repo_id = ?1");
    if !filter.statuses.is_empty() {
        sql.push_str(" AND status IN (");
        sql.push_str(&vec!["?"; filter.statuses.len()].join(", "));
        sql.push(')');
    }
    for _ in &filter.tags {
        sql.push_str(
            " AND id IN (SELECT change_number FROM change_tags WHERE key = ? AND value = ?)",
        );
    }
    sql.push_str(" ORDER BY id");
    let mut values: Vec<rusqlite::types::Value> = vec![i64::try_from(repo_id)?.into()];
    values.extend(
        filter
            .statuses
            .iter()
            .map(|s| s.as_str().to_string().into()),
    );
    for tag in &filter.tags {
        values.push(tag.key().to_string().into());
        values.push(tag.value().to_string().into());
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values), map_change)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Change tags (the denormalized cache of each change's effective tag set)

/// Replaces one change's tag rows with `tags`.
///
/// This costs a delete plus an insert per tag. A reader that caught it
/// midway would otherwise see a change holding a fragment of its set.
///
/// # Errors
///
/// On a database failure.
pub fn set_change_tags(tx: &Transaction, number: ChangeNumber, tags: &Tags) -> Result<()> {
    let id = i64::try_from(number.get())?;
    tx.execute(
        "DELETE FROM change_tags WHERE change_number = ?1",
        params![id],
    )?;
    let mut stmt =
        tx.prepare("INSERT INTO change_tags (change_number, key, value) VALUES (?1, ?2, ?3)")?;
    for (key, value) in tags.iter() {
        stmt.execute(params![id, key, value])?;
    }
    Ok(())
}

/// The distinct `key`, `value` pairs in use across one repo's changes.
///
/// The stored rows alone answer this, so nothing resolves or replays a
/// change. The pairs come back ascending, key first.
///
/// # Errors
///
/// On a database failure.
pub fn repo_tags(conn: &Connection, repo_id: u64) -> Result<Vec<(String, String)>> {
    // An `EXISTS` semi-join reads `change_tags_by_value` in its own
    // `(key, value)` order, so the distinct pairs need no sort. A plain
    // join instead probes once per change and sorts what it collects.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT t.key, t.value FROM change_tags t
         WHERE EXISTS (SELECT 1 FROM changes c WHERE c.id = t.change_number AND c.repo_id = ?1)
         ORDER BY t.key, t.value",
    )?;
    let rows = stmt.query_map(params![i64::try_from(repo_id)?], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.map(|row| Ok(row?)).collect()
}
// ---------------------------------------------------------------------------
// Log (the append-only event log, keyed on the change, globally ordered by sequence)

#[derive(Debug, Clone)]
pub struct LogRow {
    /// Globally monotone across the repo — the cross-change order.
    pub sequence: u64,
    /// 0-based, contiguous per change.
    pub position: u64,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

/// `head` = number of entries for a change = position of its next entry.
///
/// # Errors
///
/// On a database failure.
pub fn log_head(conn: &Connection, change_number: ChangeNumber) -> Result<u64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(position) FROM log WHERE change_number = ?1",
        params![i64::try_from(change_number.get())?],
        |r| r.get(0),
    )?;
    Ok(match max {
        Some(m) => col_u64(m)? + 1,
        None => 0,
    })
}

/// Appends one entry at `position`.
///
/// `position` must equal the change's current head; the caller computes it
/// under the change's projection write lock. Returns the global `sequence`
/// `SQLite` minted for the entry.
///
/// # Errors
///
/// On a database failure (including a `UNIQUE(change_number, position)` clash).
pub fn append_log(
    tx: &Transaction,
    change_number: ChangeNumber,
    position: u64,
    kind: &str,
    payload: &str,
    created_at: &str,
) -> Result<u64> {
    tx.execute(
        "INSERT INTO log (change_number, position, kind, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            i64::try_from(change_number.get())?,
            i64::try_from(position)?,
            kind,
            payload,
            created_at
        ],
    )?;
    Ok(col_u64(tx.last_insert_rowid())?)
}

fn map_log(row: &rusqlite::Row) -> rusqlite::Result<LogRow> {
    Ok(LogRow {
        sequence: col_u64(row.get("sequence")?)?,
        position: col_u64(row.get("position")?)?,
        kind: row.get("kind")?,
        payload: row.get("payload")?,
        created_at: row.get("created_at")?,
    })
}

/// One change's entries in `[from, to)`, position-ascending.
///
/// `to = None` means through head.
///
/// # Errors
///
/// On a database failure.
pub fn log_entries(
    conn: &Connection,
    change_number: ChangeNumber,
    from: u64,
    to: Option<u64>,
) -> Result<Vec<LogRow>> {
    let change_number = i64::try_from(change_number.get())?;
    let from = i64::try_from(from)?;
    // Omit the upper bound entirely rather than fake one with a sentinel.
    let rows = match to {
        Some(to) => conn
            .prepare(
                "SELECT sequence, position, kind, payload, created_at FROM log
                 WHERE change_number = ?1 AND position >= ?2 AND position < ?3 ORDER BY position",
            )?
            .query_map(params![change_number, from, i64::try_from(to)?], map_log)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => conn
            .prepare(
                "SELECT sequence, position, kind, payload, created_at FROM log
                 WHERE change_number = ?1 AND position >= ?2 ORDER BY position",
            )?
            .query_map(params![change_number, from], map_log)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Drafts (reviewer-private scratch; never enters the log)

#[derive(Debug, Clone)]
pub struct DraftRow {
    pub id: u64,
    pub change_number: ChangeNumber,
    pub revision: RevisionNumber,
    /// The thread this draft replies to; `None` opens a new thread.
    pub thread_id: Option<u64>,
    pub anchor: Anchor,
    pub body: String,
    /// Draft thread-resolution decision; `None` = none.
    ///
    /// Stored as the `resolved` INTEGER column.
    pub resolved: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

/// The anchor a draft row's location columns spell.
///
/// The four range columns win when they are set, because a selection
/// carries the lines it covers. `line` then names a whole line, and no
/// file at all is the change itself.
fn col_anchor(row: &rusqlite::Row) -> rusqlite::Result<Anchor> {
    let Some(file) = row.get::<_, Option<String>>("file")? else {
        return Ok(Anchor::Change);
    };
    let range = match (
        row.get::<_, Option<i64>>("range_start_line")?,
        row.get::<_, Option<i64>>("range_start_char")?,
        row.get::<_, Option<i64>>("range_end_line")?,
        row.get::<_, Option<i64>>("range_end_char")?,
    ) {
        (Some(start_line), Some(start_char), Some(end_line), Some(end_char)) => Some(stored(
            CommentRange::new(
                col_u64(start_line)?,
                col_u64(start_char)?,
                col_u64(end_line)?,
                col_u64(end_char)?,
            ),
            rusqlite::types::Type::Integer,
        )?),
        _ => None,
    };
    let at = match (range, col_u64_opt(row.get("line")?)?) {
        (Some(range), _) => LineAnchor::Selection(range),
        (None, Some(line)) => LineAnchor::Whole(line),
        (None, None) => return Ok(Anchor::File { file }),
    };
    Ok(Anchor::Line {
        file,
        side: col_enum(&row.get::<_, String>("side")?)?,
        line_text: row.get("line_text")?,
        at,
    })
}

fn map_draft(row: &rusqlite::Row) -> rusqlite::Result<DraftRow> {
    Ok(DraftRow {
        id: col_u64(row.get("id")?)?,
        change_number: ChangeNumber::new(col_u64(row.get("change_number")?)?),
        revision: RevisionNumber::new(col_u64(row.get("revision")?)?),
        thread_id: col_u64_opt(row.get("thread_id")?)?,
        anchor: col_anchor(row)?,
        body: row.get("body")?,
        resolved: row.get::<_, Option<i64>>("resolved")?.map(|v| v != 0),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub struct NewDraft<'a> {
    pub change_number: ChangeNumber,
    pub revision: RevisionNumber,
    pub thread_id: Option<u64>,
    pub anchor: &'a Anchor,
    pub body: &'a str,
    pub resolved: Option<bool>,
}

/// Inserts a draft with a caller-allocated `id`.
///
/// The id comes from the server's global fold-id counter, so a draft's id
/// stays stable when it later publishes into a `review` entry — and never
/// collides with any other id.
///
/// # Errors
///
/// On a database failure.
pub fn insert_draft(tx: &Transaction, id: u64, d: &NewDraft, now: &str) -> Result<DraftRow> {
    let thread_id = d.thread_id.map(i64::try_from).transpose()?;
    let at = match d.anchor {
        Anchor::Line { at, .. } => Some(*at),
        _ => None,
    };
    let line = match at {
        Some(LineAnchor::Whole(line)) => Some(i64::try_from(line)?),
        _ => None,
    };
    let (rsl, rsc, rel, rec) = match at {
        Some(LineAnchor::Selection(r)) => (
            Some(i64::try_from(r.start_line())?),
            Some(i64::try_from(r.start_char())?),
            Some(i64::try_from(r.end_line())?),
            Some(i64::try_from(r.end_char())?),
        ),
        _ => (None, None, None, None),
    };
    tx.execute(
        "INSERT INTO draft_comments (id, change_number, revision, thread_id, file, line, side,
            range_start_line, range_start_char, range_end_line, range_end_char,
            line_text, body, resolved, created_at, updated_at)
         VALUES (?14, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?15, ?13, ?13)",
        params![
            i64::try_from(d.change_number.get())?,
            i64::try_from(d.revision.get())?,
            thread_id,
            d.anchor.file(),
            line,
            d.anchor.side().as_str(),
            rsl,
            rsc,
            rel,
            rec,
            d.anchor.line_text(),
            d.body,
            now,
            i64::try_from(id)?,
            d.resolved.map(i64::from),
        ],
    )?;
    get_draft(tx, id)?.ok_or_else(|| anyhow!("draft {id} vanished"))
}

/// The maximum draft id, for seeding the global id counter on startup.
///
/// # Errors
///
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
///
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

/// # Errors
///
/// On a database failure.
pub fn update_draft(
    tx: &Transaction,
    id: u64,
    body: &str,
    resolved: Option<bool>,
    now: &str,
) -> Result<()> {
    tx.execute(
        "UPDATE draft_comments SET body = ?1, resolved = ?4, updated_at = ?2 WHERE id = ?3",
        params![body, now, i64::try_from(id)?, resolved.map(i64::from)],
    )?;
    Ok(())
}

/// Whether a draft was there to delete.
///
/// # Errors
///
/// On a database failure.
pub fn delete_draft(tx: &Transaction, id: u64) -> Result<bool> {
    let deleted = tx.execute(
        "DELETE FROM draft_comments WHERE id = ?1",
        params![i64::try_from(id)?],
    )?;
    Ok(deleted > 0)
}

/// Id-ascending (creation order).
///
/// # Errors
///
/// On a database failure.
pub fn drafts_for_change(conn: &Connection, change_number: ChangeNumber) -> Result<Vec<DraftRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM draft_comments WHERE change_number = ?1 ORDER BY id")?;
    let rows = stmt
        .query_map(params![i64::try_from(change_number.get())?], map_draft)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Called when a change's drafts publish.
///
/// # Errors
///
/// On a database failure.
pub fn delete_drafts_for_change(tx: &Transaction, change_number: ChangeNumber) -> Result<()> {
    tx.execute(
        "DELETE FROM draft_comments WHERE change_number = ?1",
        params![i64::try_from(change_number.get())?],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Draft reviews: never written to the log until published
// (`crate::api::reviews`).

/// A reviewer's draft decision on a change.
#[derive(Debug, Clone)]
pub struct DraftReviewRow {
    pub change_number: ChangeNumber,
    pub decision: Decision,
    /// Cover note (for a verdict) or reason (for `abandon`).
    pub message: String,
}

fn map_draft_review(row: &rusqlite::Row) -> rusqlite::Result<DraftReviewRow> {
    Ok(DraftReviewRow {
        change_number: ChangeNumber::new(col_u64(row.get("change_number")?)?),
        decision: col_enum(&row.get::<_, String>("decision")?)?,
        message: row.get("message")?,
    })
}

/// Sets (or overwrites) a change's draft decision.
///
/// One row per change: a later write replaces the prior decision and
/// message.
///
/// # Errors
///
/// On a database failure.
pub fn upsert_draft_review(
    tx: &Transaction,
    change_number: ChangeNumber,
    decision: Decision,
    message: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO draft_reviews (change_number, decision, message) VALUES (?1, ?2, ?3)
         ON CONFLICT (change_number) DO UPDATE SET decision = ?2, message = ?3",
        params![
            i64::try_from(change_number.get())?,
            decision.as_str(),
            message
        ],
    )?;
    Ok(())
}

/// The change's draft decision, if any.
///
/// # Errors
///
/// On a database failure.
pub fn get_draft_review(
    conn: &Connection,
    change_number: ChangeNumber,
) -> Result<Option<DraftReviewRow>> {
    conn.query_row(
        "SELECT * FROM draft_reviews WHERE change_number = ?1",
        params![i64::try_from(change_number.get())?],
        map_draft_review,
    )
    .optional()
    .map_err(Into::into)
}

/// Discards a change's draft decision.
///
/// Called when it publishes, or on an explicit clear. A no-op when
/// nothing is drafted.
///
/// # Errors
///
/// On a database failure.
pub fn delete_draft_review(tx: &Transaction, change_number: ChangeNumber) -> Result<()> {
    tx.execute(
        "DELETE FROM draft_reviews WHERE change_number = ?1",
        params![i64::try_from(change_number.get())?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nit_types::domain::{CommentRange, LineAnchor, Side};
    use nit_types::testing::{change_id, sha, tag, tags};

    fn mem() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory db");
        conn.pragma_update(None, "foreign_keys", "ON").expect("fk");
        migrate(&mut conn).expect("migrate");
        conn
    }

    fn repo(conn: &mut Connection, git_dir: &str) -> RepoRow {
        write(conn, |tx| create_repo(tx, git_dir, "main")).expect("repo")
    }

    fn change(conn: &mut Connection) -> ChangeNumber {
        let repo = repo(conn, "/r/.git");
        change_in(conn, repo.id, "I1")
    }

    fn change_in(conn: &mut Connection, repo_id: u64, label: &str) -> ChangeNumber {
        write(conn, |tx| upsert_change(tx, repo_id, &change_id(label))).expect("change")
    }

    fn matching_tags(conn: &Connection, repo_id: u64, tags: Vec<Tag>) -> Vec<ChangeNumber> {
        repo_changes(
            conn,
            repo_id,
            &ChangeFilter {
                tags,
                ..ChangeFilter::default()
            },
        )
        .expect("query")
        .into_iter()
        .map(|row| row.id)
        .collect()
    }

    fn set_tags(conn: &mut Connection, number: ChangeNumber, pairs: &[(&str, &str)]) {
        write(conn, |tx| set_change_tags(tx, number, &tags(pairs))).expect("set");
    }

    #[test]
    fn change_tags_replace_wholesale() {
        let mut conn = mem();
        let repo = repo(&mut conn, "/r/.git");
        let id = change_in(&mut conn, repo.id, "I1");
        set_tags(&mut conn, id, &[("branch", "track/a"), ("feature", "epic")]);
        set_tags(&mut conn, id, &[("branch", "track/b")]);

        let matching = |t: Tag| matching_tags(&conn, repo.id, vec![t]);
        assert_eq!(matching(tag("branch", "track/b")), vec![id]);
        assert!(matching(tag("branch", "track/a")).is_empty());
        assert!(
            matching(tag("feature", "epic")).is_empty(),
            "a key the second write left out is gone, not carried"
        );

        set_tags(&mut conn, id, &[]);
        assert!(
            matching_tags(&conn, repo.id, vec![tag("branch", "track/b")]).is_empty(),
            "clearing the set leaves the change matching nothing"
        );
    }

    #[test]
    fn repo_changes_and_every_requested_tag() {
        let mut conn = mem();
        let repo = repo(&mut conn, "/r/.git");
        let both = change_in(&mut conn, repo.id, "I1");
        let one = change_in(&mut conn, repo.id, "I2");
        set_tags(
            &mut conn,
            both,
            &[("branch", "track/a"), ("feature", "epic")],
        );
        set_tags(&mut conn, one, &[("branch", "track/a")]);

        let matching = |tags| matching_tags(&conn, repo.id, tags);
        assert_eq!(matching(vec![]), vec![both, one]);
        assert_eq!(matching(vec![tag("branch", "track/a")]), vec![both, one]);
        assert_eq!(
            matching(vec![tag("branch", "track/a"), tag("feature", "epic")]),
            vec![both]
        );
        assert!(matching(vec![tag("feature", "other")]).is_empty());
    }

    #[test]
    fn create_repo_registers_and_find_locates() {
        let mut conn = mem();
        let a = repo(&mut conn, "/r/.git");
        assert_eq!(a.canonical_ref, "main");
        let found = find_repo(&conn, "/r/.git").expect("query").expect("found");
        assert_eq!(found.id, a.id);
        assert_eq!(found.canonical_ref, "main");
        let b = repo(&mut conn, "/other/.git");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn canonical_head_starts_null_and_round_trips() {
        let mut conn = mem();
        let a = repo(&mut conn, "/r/.git");
        assert_eq!(a.canonical_head, None, "no baseline until first observed");
        write(&mut conn, |tx| {
            update_repo_canonical_head(tx, a.id, &sha("deadbeef"))
        })
        .expect("record");
        let found = find_repo(&conn, "/r/.git").expect("query").expect("found");
        assert_eq!(found.canonical_head, Some(sha("deadbeef")));
    }

    /// The location columns spell the three anchors, and each one has to
    /// come back as the anchor it spells.
    #[test]
    fn draft_rows_read_back_as_their_anchors() {
        let mut conn = mem();
        let c = change(&mut conn);
        let anchors = [
            Anchor::Line {
                file: "a.rs".to_string(),
                side: Side::Old,
                line_text: Some("x1".to_string()),
                at: LineAnchor::Whole(3),
            },
            Anchor::Line {
                file: "a.rs".to_string(),
                side: Side::New,
                line_text: Some("x2".to_string()),
                at: LineAnchor::Selection(
                    CommentRange::new(4, 1, 4, 6).expect("a forward selection"),
                ),
            },
            Anchor::File {
                file: "a.rs".to_string(),
            },
            Anchor::Change,
        ];
        for (id, anchor) in anchors.iter().enumerate() {
            write(&mut conn, |tx| {
                insert_draft(
                    tx,
                    id as u64,
                    &NewDraft {
                        change_number: c,
                        revision: RevisionNumber::new(0),
                        thread_id: None,
                        anchor,
                        body: "note",
                        resolved: None,
                    },
                    "t0",
                )
            })
            .expect("insert");
        }

        let read: Vec<Anchor> = drafts_for_change(&conn, c)
            .expect("list")
            .into_iter()
            .map(|d| d.anchor)
            .collect();
        assert_eq!(read, anchors);
    }

    /// v9 renames a change column on four tables; the rows written under
    /// the old names have to come back under the new ones.
    #[test]
    fn v9_renames_the_change_columns_over_existing_rows() {
        let mut conn = Connection::open_in_memory().expect("in-memory db");
        // Stop at v8 and write rows the way that schema spells them.
        migrate_to(&mut conn, 8).expect("migrate to v8");
        let key = change_id("Iabc");
        conn.execute_batch(&format!(
            "INSERT INTO repos (id, git_dir, canonical_ref) VALUES (1, '/r/.git', 'main');
             INSERT INTO changes (id, repo_id, change_key, created_at)
                  VALUES (7, 1, '{key}', 't');
             INSERT INTO log (change_id, position, kind, payload, created_at)
                  VALUES (7, 0, 'revision', '{{}}', 't');
             INSERT INTO draft_comments (change_id, revision, body, created_at, updated_at)
                  VALUES (7, 0, 'note', 't', 't');
             INSERT INTO draft_reviews (change_id, decision, message)
                  VALUES (7, 'approve', '');"
        ))
        .expect("v8-shaped rows");

        migrate(&mut conn).expect("migrate the rest");

        let change = get_change(&conn, ChangeNumber::new(7))
            .expect("get")
            .expect("still there");
        assert_eq!(change.change_id, key);
        let logged: i64 = conn
            .query_row("SELECT change_number FROM log", [], |r| r.get(0))
            .expect("log row");
        assert_eq!(logged, 7);
        for table in ["draft_comments", "draft_reviews"] {
            let drafted: i64 = conn
                .query_row(&format!("SELECT change_number FROM {table}"), [], |r| {
                    r.get(0)
                })
                .unwrap_or_else(|e| panic!("{table} row: {e}"));
            assert_eq!(drafted, 7);
        }
    }

    #[test]
    fn change_upsert_is_idempotent() {
        let mut conn = mem();
        let repo = repo(&mut conn, "/r/.git");
        let a = change_in(&mut conn, repo.id, "Iabc");
        let again = change_in(&mut conn, repo.id, "Iabc");
        assert_eq!(a, again);
        let b = change_in(&mut conn, repo.id, "Idef");
        assert_ne!(a, b);
        assert_eq!(
            get_change(&conn, a).expect("get").expect("some").change_id,
            change_id("Iabc")
        );
    }

    #[test]
    fn change_status_round_trips() {
        let mut conn = mem();
        let c = change(&mut conn);
        let status = |conn: &Connection| -> Option<String> {
            conn.query_row(
                "SELECT status FROM changes WHERE id = ?1",
                params![i64::try_from(c.get()).expect("id fits i64")],
                |r| r.get(0),
            )
            .expect("query status")
        };
        // The insert writes the status, so a change with no append still
        // filters as what its empty fold says it is.
        assert_eq!(status(&conn).as_deref(), Some("pending"));
        assert_eq!(
            get_change(&conn, c).expect("get").expect("row").status,
            Some(ChangeStatus::Pending)
        );
        write(&mut conn, |tx| {
            update_change_status(tx, c, ChangeStatus::Approved)
        })
        .expect("stamp");
        assert_eq!(status(&conn).as_deref(), Some("approved"));
        write(&mut conn, |tx| {
            update_change_status(tx, c, ChangeStatus::ChangesRequested)
        })
        .expect("restamp");
        assert_eq!(status(&conn).as_deref(), Some("changes_requested"));
        assert_eq!(
            get_change(&conn, c).expect("get").expect("row").status,
            Some(ChangeStatus::ChangesRequested)
        );
    }

    #[test]
    fn log_append_mints_sequence_and_position() {
        let mut conn = mem();
        let c = change(&mut conn);
        assert_eq!(log_head(&conn, c).expect("head"), 0);
        let s0 = write(&mut conn, |tx| {
            append_log(tx, c, 0, "revision", r#"{"commit_sha":"a"}"#, "t0")
        })
        .expect("append");
        let s1 = write(&mut conn, |tx| {
            append_log(tx, c, 1, "comment", r#"{"body":"note"}"#, "t1")
        })
        .expect("append");
        assert!(s1 > s0, "sequence is monotone");
        assert_eq!(log_head(&conn, c).expect("head"), 2);
        let entries = log_entries(&conn, c, 0, None).expect("entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, "revision");
        assert_eq!(entries[1].position, 1);
        let tail = log_entries(&conn, c, 1, None).expect("tail");
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].kind, "comment");
    }

    #[test]
    fn sequence_is_global_across_changes() {
        let mut conn = mem();
        let repo = repo(&mut conn, "/r/.git");
        let a = change_in(&mut conn, repo.id, "Ia");
        let b = change_in(&mut conn, repo.id, "Ib");
        let sa = write(&mut conn, |tx| append_log(tx, a, 0, "comment", "{}", "t0")).expect("a0");
        let sb = write(&mut conn, |tx| append_log(tx, b, 0, "comment", "{}", "t1")).expect("b0");
        let sa1 = write(&mut conn, |tx| append_log(tx, a, 1, "comment", "{}", "t2")).expect("a1");
        // Both changes' position restart at 0, but sequence totally orders the interleave.
        assert!(sa < sb && sb < sa1);
    }

    #[test]
    fn draft_lifecycle() {
        let mut conn = mem();
        let c = change(&mut conn);
        let d = write(&mut conn, |tx| {
            insert_draft(
                tx,
                7,
                &NewDraft {
                    change_number: c,
                    revision: RevisionNumber::new(1),
                    thread_id: None,
                    anchor: &Anchor::Line {
                        file: "src/main.rs".to_string(),
                        side: Side::New,
                        line_text: Some("fn main".to_string()),
                        at: LineAnchor::Whole(3),
                    },
                    body: "look",
                    resolved: None,
                },
                "t0",
            )
        })
        .expect("insert");
        assert_eq!(drafts_for_change(&conn, c).expect("list").len(), 1);
        write(&mut conn, |tx| {
            update_draft(tx, d.id, "look again", Some(true), "t1")
        })
        .expect("edit");
        let edited = get_draft(&conn, d.id).expect("get").expect("some");
        assert_eq!(edited.body, "look again");
        assert_eq!(edited.resolved, Some(true));
        write(&mut conn, |tx| delete_drafts_for_change(tx, c)).expect("drain");
        assert!(drafts_for_change(&conn, c).expect("list").is_empty());
    }

    #[test]
    fn draft_review_upsert_get_delete() {
        let mut conn = mem();
        let c = change(&mut conn);
        assert!(get_draft_review(&conn, c).expect("get").is_none());

        write(&mut conn, |tx| {
            upsert_draft_review(tx, c, Decision::RequestChanges, "fix this")
        })
        .expect("draft");
        let draft = get_draft_review(&conn, c).expect("get").expect("some");
        assert_eq!(draft.decision, Decision::RequestChanges);
        assert_eq!(draft.message, "fix this");

        write(&mut conn, |tx| {
            upsert_draft_review(tx, c, Decision::Approve, "lgtm")
        })
        .expect("redraft");
        let draft = get_draft_review(&conn, c).expect("get").expect("some");
        assert_eq!(draft.decision, Decision::Approve);
        assert_eq!(draft.message, "lgtm");

        write(&mut conn, |tx| delete_draft_review(tx, c)).expect("clear");
        assert!(get_draft_review(&conn, c).expect("get").is_none());
        write(&mut conn, |tx| delete_draft_review(tx, c)).expect("clear again");
    }
}
