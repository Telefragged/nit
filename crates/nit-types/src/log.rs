//! The log read across changes.

use serde::{Deserialize, Serialize};

use crate::domain::LogEntry;

/// Log entries from several changes, ascending by global `sequence`.
///
/// `GET /api/log` returns the entries of every change a filter matches.
/// `repo` is required. `status` and `tag` select changes exactly as they
/// do on `GET /api/changes`. `after` and `before` are `sequence` bounds,
/// both exclusive, and each is optional. So `?after=n` returns every
/// entry with `sequence > n`. The filter uses the tags a change has now,
/// not the tags it had when each entry was written. This matters because
/// `nit push` writes the revision entry first and the `tags` entry second,
/// and a client must still get the revision entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    pub entries: Vec<LogEntry>,
}
