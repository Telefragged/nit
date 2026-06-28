//! The spine-centered change graph (docs/api.md "Graph").

use serde::{Deserialize, Serialize};

use crate::enums::{ChangeStatus, GraphSection};

/// One repo's change graph: a single commit-sha-keyed DAG over the canonical
/// branch, the source for the web dashboard. Nothing about it is stored — it
/// is assembled at read time from the same folds + sha index as a chain, plus
/// a git walk of the canonical branch for the merged history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoGraph {
    pub repo_id: u64,
    /// The HEAD node's `commit_sha` — the anchor every region pivots on.
    pub anchor: String,
    /// The canonical branch has merged commits below the displayed window — the
    /// client shows an "earlier history hidden" marker and dangles deep forks
    /// to it.
    pub history_truncated: bool,
    /// Row order, top → bottom: open (top) → head → history (bottom). A
    /// topological order in which every node precedes its parents.
    pub nodes: Vec<GraphNode>,
}

/// One node of the change graph, keyed by its `commit_sha`. Edges are its
/// `parents` (an edge is drawn to each that is in the node set; `len > 1` is
/// a merge).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GraphNode {
    /// The node's stable id — a full 40-hex commit-sha; the client truncates.
    pub commit_sha: String,
    pub section: GraphSection,
    pub subject: String,
    /// `ChangeStatus` at the pinned revision; head/history read as merged —
    /// the client styles by `section`.
    pub status: ChangeStatus,
    /// Parent commit-shas; an edge is drawn to each that is in the node set.
    pub parents: Vec<String>,
    /// The backing change, or `None` for a bare git commit (merge / pre-nit).
    pub change_id: Option<u64>,
    pub change_key: Option<String>,
    /// The pinned patchset (open nodes); `None` off the open region.
    pub revision: Option<u64>,
}
