//! The per-change fold and its server-side adapters.
//!
//! The fold itself ([`nit_types::fold`], re-exported here so the rest of the
//! crate keeps importing `crate::review::*`) is pure over `nit_types` — no
//! database, no storage serialization, no event publishing — so the same code
//! folds the websocket stream client-side once compiled to WebAssembly. This
//! module adds the two things that must **not** follow it to the browser: the
//! storage boundary (`payload_to_json`/`payload_from_json`, the
//! `log.payload` column split) and the row adapters that build wire
//! [`LogEntry`]s from `db` rows (`entry_from_row`/[`replay_rows`]).

use anyhow::{Result, anyhow};

use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::LogKind;
use nit_types::domain::{LogEntry, LogPayload};

use crate::db;

pub use nit_types::fold::*;

/// Serializes a log payload to the JSON in its `log.payload` column.
///
/// The stored JSON is the inner struct alone, since the entry's `kind` is
/// stored in its own column. The write half of the storage boundary
/// ([`payload_from_json`] is the read half) — no `serde_json::Value`
/// crosses it.
///
/// # Errors
///
/// When the payload fails to serialize — impossible for these plain structs.
pub(crate) fn payload_to_json(payload: &LogPayload) -> Result<String> {
    match payload {
        LogPayload::Revision(p) => serde_json::to_string(p),
        LogPayload::Review(p) => serde_json::to_string(p),
        LogPayload::Comment(p) => serde_json::to_string(p),
        LogPayload::Lifecycle(p) => serde_json::to_string(p),
        LogPayload::Tags(p) => serde_json::to_string(p),
        LogPayload::Attach(p) => serde_json::to_string(p),
    }
    .map_err(Into::into)
}

/// Parses a stored entry's `kind` + inner JSON into the typed payload.
///
/// The read half of the storage boundary (mirrors [`payload_to_json`]).
///
/// # Errors
///
/// When the JSON does not match the payload shape for `kind`.
pub(crate) fn payload_from_json(kind: LogKind, json: &str) -> Result<LogPayload> {
    Ok(match kind {
        LogKind::Revision => LogPayload::Revision(serde_json::from_str(json)?),
        LogKind::Review => LogPayload::Review(serde_json::from_str(json)?),
        LogKind::Comment => LogPayload::Comment(serde_json::from_str(json)?),
        LogKind::Lifecycle => LogPayload::Lifecycle(serde_json::from_str(json)?),
        LogKind::Tags => LogPayload::Tags(serde_json::from_str(json)?),
        LogKind::Attach => LogPayload::Attach(serde_json::from_str(json)?),
    })
}

/// The entries of a change with `from <= position < to`, ascending by
/// position, as wire entries.
///
/// A `None` end means no bound on that side.
///
/// # Errors
///
/// On a database failure, or a stored row that is not an entry.
pub fn entries_from(
    conn: &rusqlite::Connection,
    change: ChangeNumber,
    from: u64,
    to: Option<u64>,
) -> Result<Vec<LogEntry>> {
    db::log_entries_from(conn, change, from, to)?
        .iter()
        .map(|row| entry_from_row(change, row))
        .collect()
}

/// Converts a stored log row into the wire [`LogEntry`] that the fold reads.
///
/// The stream sends this same entry. The caller passes the `change_number`,
/// because a [`db::LogRow`] contains the per-change coordinates alone.
///
/// # Errors
///
/// When the stored `kind` is unknown or the payload is not valid JSON.
fn entry_from_row(change_number: ChangeNumber, row: &db::LogRow) -> Result<LogEntry> {
    let kind: LogKind = row
        .kind
        .parse()
        .map_err(|e| anyhow!("log entry {}: {e}", row.position))?;
    Ok(LogEntry {
        change_number,
        position: row.position,
        created_at: row.created_at.clone(),
        payload: payload_from_json(kind, &row.payload)
            .map_err(|e| anyhow!("log entry {}: bad payload: {e}", row.position))?,
    })
}

/// Replays one change from its log.
///
/// `None` when no change has the number.
///
/// # Errors
///
/// On a database failure, or a log payload that fails to parse.
pub fn load(conn: &rusqlite::Connection, change: ChangeNumber) -> Result<Option<ChangeProjection>> {
    db::get_change(conn, change)?
        .map(|row| load_row(conn, &row))
        .transpose()
}

/// Replays the projections of one repo's changes that `filter` picks.
///
/// `filter` narrows in the database (the denormalized `changes.status`
/// column and `change_tags`), so nothing replays a change outside it.
///
/// # Errors
///
/// On a database failure, or a log payload that fails to parse.
pub fn repo_changes(
    conn: &rusqlite::Connection,
    repo_id: u64,
    filter: &db::ChangeFilter,
) -> Result<Vec<ChangeProjection>> {
    db::repo_changes(conn, repo_id, filter)?
        .iter()
        .map(|row| load_row(conn, row))
        .collect()
}

fn load_row(conn: &rusqlite::Connection, row: &db::ChangeRow) -> Result<ChangeProjection> {
    replay_rows(row, &db::log_entries(conn, row.id)?)
}

/// Rebuilds a change's projection from its row and its log rows.
///
/// The log rows are in ascending position order.
///
/// # Errors
///
/// When a log payload fails to parse.
pub fn replay_rows(row: &db::ChangeRow, rows: &[db::LogRow]) -> Result<ChangeProjection> {
    let entries = rows
        .iter()
        .map(|r| entry_from_row(row.id, r))
        .collect::<Result<Vec<_>>>()?;
    Ok(replay(row.id, row.repo_id, row.change_id.clone(), entries))
}

#[cfg(test)]
mod tests;
