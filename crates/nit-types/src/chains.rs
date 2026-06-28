//! Derived chains: the path through a tip change plus its rolled-up state
//! (docs/api.md "Chains").

use serde::{Deserialize, Serialize};

use crate::enums::{ChainState, ChangeStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainList {
    pub chains: Vec<Chain>,
}

/// A derived chain: the path through a tip change, plus its rolled-up state.
/// The list element (`GET /api/chains`) and the single-chain shape
/// (`GET /api/chains/{id}`) are identical.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Chain {
    pub tip_change_id: u64,
    /// The repo this chain belongs to (registry id).
    pub repo_id: u64,
    pub state: ChainState,
    /// Oldest-first, base → tip.
    pub path: Vec<PathEntry>,
}

/// One member of a derived path: structure only, read at the revision the path
/// pins. Per-change review state (counts, staged decision, the newest patchset)
/// is not here — a client reads it from `GET /api/changes/{id}` per member.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PathEntry {
    pub change_id: u64,
    /// Position in THIS path (0-based).
    pub position: u64,
    pub change_key: String,
    /// The patchset this path walks.
    pub revision: u64,
    /// Per `(change, this revision)`.
    pub status: ChangeStatus,
    pub subject: String,
    pub commit_sha: String,
}
