//! Derived chains: the path through a tip change plus its rolled-up state.

use serde::{Deserialize, Serialize};

use crate::domain::ChangeId;
use crate::domain::Sha;
use crate::domain::{ChainState, ChangeStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainList {
    pub chains: Vec<Chain>,
}

/// A derived chain: a tip change's path plus its rolled-up state.
///
/// The list element (`GET /api/chains`) and the single-chain shape
/// (`GET /api/chains/{id}`) are identical.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Chain {
    pub tip_change_id: u64,
    pub repo_id: u64,
    pub state: ChainState,
    /// Oldest-first, base → tip.
    pub path: Vec<PathEntry>,
}

/// One member of a derived path: structure only.
///
/// Read at the revision the path pins. Per-change review state (counts,
/// draft decision, the newest revision) is not here — a client reads it
/// from `GET /api/changes/{id}` per member.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PathEntry {
    pub change_id: u64,
    /// Position in THIS path (0-based).
    pub position: u64,
    pub change_key: ChangeId,
    /// The revision this path walks.
    pub revision: u64,
    /// Per `(change, this revision)`.
    pub status: ChangeStatus,
    pub subject: String,
    pub commit_sha: Sha,
}
