//! A chain: the path a tip change walks back to the canonical ref.

use serde::{Deserialize, Serialize};

use super::ChainState;
use super::ChangeId;
use super::ChangeNumber;
use super::ChangeStatus;
use super::RevisionNumber;
use super::Sha;

/// A derived chain: a tip change's path plus its rolled-up state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Chain {
    pub tip_change_number: ChangeNumber,
    pub repo_id: u64,
    pub state: ChainState,
    /// Oldest-first, base → tip.
    pub path: Vec<PathEntry>,
}

/// One member of a derived path: structure only.
///
/// Read at the revision the path pins. Per-change review state (counts,
/// draft decision, the newest revision) is not here — it belongs to the
/// change itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PathEntry {
    pub change_number: ChangeNumber,
    /// Position in THIS path (0-based).
    pub position: u64,
    pub change_id: ChangeId,
    /// The revision this path walks.
    pub revision: RevisionNumber,
    /// Per `(change, this revision)`.
    pub status: ChangeStatus,
    pub subject: String,
    pub commit_sha: Sha,
}
