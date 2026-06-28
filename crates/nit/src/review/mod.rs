//! The per-change fold and its server-side adapters.
//!
//! The fold itself ([`nit_types::fold`], re-exported here so the rest of the
//! crate keeps importing `crate::review::*`) is pure over `nit_types` — no
//! database, no storage serialization, no event publishing — so the same code
//! folds the websocket stream client-side once compiled to WebAssembly. This
//! module adds the two things that must **not** follow it to the browser: the
//! storage boundary ([`payload_to_json`]/[`payload_from_json`], the
//! `log.payload` column split) and the row adapters that build wire
//! [`LogEntry`]s from `db` rows ([`entry_from_row`]/[`replay_rows`]).

use anyhow::{Result, anyhow};

use nit_types::enums::LogKind;
use nit_types::log::{LogEntry, LogPayload};

use crate::db;

pub use nit_types::fold::*;

/// Serialize a log payload to the JSON stored in its `log.payload` column: the
/// inner struct alone, since the entry's `kind` is stored in its own column.
/// The write half of the storage boundary ([`payload_from_json`] is the read
/// half) — no `serde_json::Value` crosses it.
///
/// # Errors
/// When the payload fails to serialize — impossible for these plain structs.
pub(crate) fn payload_to_json(payload: &LogPayload) -> Result<String> {
    match payload {
        LogPayload::Revision(p) => serde_json::to_string(p),
        LogPayload::Review(p) => serde_json::to_string(p),
        LogPayload::Comment(p) => serde_json::to_string(p),
        LogPayload::Lifecycle(p) => serde_json::to_string(p),
    }
    .map_err(Into::into)
}

/// Parse a stored entry's `kind` + inner JSON back into the typed payload — the
/// read half of the storage boundary (mirrors [`payload_to_json`]).
///
/// # Errors
/// When the JSON does not match the payload shape for `kind`.
pub(crate) fn payload_from_json(kind: LogKind, json: &str) -> Result<LogPayload> {
    Ok(match kind {
        LogKind::Revision => LogPayload::Revision(serde_json::from_str(json)?),
        LogKind::Review => LogPayload::Review(serde_json::from_str(json)?),
        LogKind::Comment => LogPayload::Comment(serde_json::from_str(json)?),
        LogKind::Lifecycle => LogPayload::Lifecycle(serde_json::from_str(json)?),
    })
}

/// A stored log row → the wire [`LogEntry`] the fold consumes (and the server
/// broadcasts). The `change_id` is the caller's — a row knows only its own
/// per-change coordinates.
///
/// # Errors
/// When the stored `kind` is unknown or the payload is not valid JSON.
pub fn entry_from_row(change_id: u64, row: &db::LogRow) -> Result<LogEntry> {
    let kind: LogKind = row
        .kind
        .parse()
        .map_err(|e| anyhow!("log entry {}: {e}", row.idx))?;
    Ok(LogEntry {
        change_id,
        idx: row.idx,
        seq: row.seq,
        created_at: row.created_at.clone(),
        payload: payload_from_json(kind, &row.payload)
            .map_err(|e| anyhow!("log entry {}: bad payload: {e}", row.idx))?,
    })
}

/// Rebuild a change's projection from its row and its log rows (ascending idx).
///
/// # Errors
/// When a log payload fails to parse.
pub fn replay_rows(row: &db::ChangeRow, rows: &[db::LogRow]) -> Result<ChangeProj> {
    let entries = rows
        .iter()
        .map(|r| entry_from_row(row.id, r))
        .collect::<Result<Vec<_>>>()?;
    Ok(replay(row.id, row.repo_id, row.change_key.clone(), entries))
}

#[cfg(test)]
mod tests;
