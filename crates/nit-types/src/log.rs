//! The log read of a change.

use serde::{Deserialize, Serialize};

use crate::domain::LogEntry;

/// The log of a change: `GET /api/changes/{id}/log` returns its entries,
/// ascending by `position`.
///
/// `from` and `to` select the entries with `from <= position < to`. Each
/// is optional, so `?from=n` returns every entry from position `n` on.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Log {
    pub entries: Vec<LogEntry>,
}
