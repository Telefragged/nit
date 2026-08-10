//! The chain endpoints' response bodies.

use serde::{Deserialize, Serialize};

use crate::domain::Chain;
use crate::domain::LogEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainList {
    pub chains: Vec<Chain>,
}

/// `GET /api/chains/{change_id}/log` response.
///
/// The aggregated chain log, merged across members and sorted by global
/// `sequence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainLog {
    pub entries: Vec<LogEntry>,
}
