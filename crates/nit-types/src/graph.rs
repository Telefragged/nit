//! The change graph, centered on the canonical ref.

use serde::{Deserialize, Serialize};

use crate::domain::ChangeId;
use crate::domain::ChangeNumber;
use crate::domain::RevisionNumber;
use crate::domain::Sha;
use crate::domain::{ChangeStatus, GraphSection};

/// One commit of the canonical ref's merged history.
///
/// Walked from the tracked ref's HEAD down (`GET /api/history?repo={id}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HistoryCommit {
    pub sha: Sha,
    /// Parent commit-shas; more than one is a merge.
    pub parents: Vec<Sha>,
    pub subject: String,
    /// The merged change this commit carries, matched by its `Change-Id:`
    /// trailer. Coupled with `change_id`: a commit whose trailer names no
    /// known change (a merge, a pre-nit commit, a foreign trailer) reports
    /// both as `None`, never an orphan key.
    pub change_number: Option<ChangeNumber>,
    pub change_id: Option<ChangeId>,
}

/// A window of the canonical ref's merged history (`GET /api/history`).
///
/// The tracked ref's HEAD first, then its ancestors, a **fixed window of 5
/// commits** deep.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoHistory {
    /// HEAD-first; each commit's `parents` carry the edges.
    pub commits: Vec<HistoryCommit>,
    /// The branch has more merged commits below the window.
    pub truncated: bool,
}

/// One repo's change graph: a commit-sha-keyed DAG over the canonical ref.
///
/// Not a response body — the browser assembles it (`crates/nit-wasm`) from
/// the two primitive reads, `GET /api/changes` and `GET /api/history`; the
/// shape lives here because it crosses the wasm↔JS boundary.
///
/// The caller may group the graph by one tag key. Open nodes that carry
/// the same value for that key then sit in one run of rows. Each node
/// reports its own value as `group`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoGraph {
    /// The canonical ref has merged commits below the displayed window — the
    /// client shows an "earlier history hidden" marker and dangles deep forks
    /// to it.
    pub history_truncated: bool,
    /// Row order, top → bottom: open (top) → head → history (bottom).
    ///
    /// A topological order in which every node precedes its parents. In a
    /// grouped graph, nodes of one group are adjacent wherever that order
    /// allows. A node of another group interrupts a run only when the
    /// topological order puts it between two nodes of that run.
    pub nodes: Vec<GraphNode>,
}

/// One node of the change graph, keyed by its `commit_sha`.
///
/// Edges are its `parents` (an edge is drawn to each that is in the node
/// set; `len > 1` is a merge). An open node whose parent is not in the
/// set attaches to its `fork_sha` instead. The commits between the two
/// are not in the graph. When the parent is the fork, the base is older
/// than the displayed window, and nothing is missing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GraphNode {
    /// The node's stable id.
    pub commit_sha: Sha,
    pub section: GraphSection,
    pub subject: String,
    /// `ChangeStatus` at the pinned revision; head/history read as merged.
    ///
    /// The client styles by `section`.
    pub status: ChangeStatus,
    /// Parent commit-shas; an edge is drawn to each that is in the node set.
    pub parents: Vec<Sha>,
    /// The backing change, or `None` for a bare git commit (merge / pre-nit).
    pub change_number: Option<ChangeNumber>,
    pub change_id: Option<ChangeId>,
    /// The pinned revision (open nodes); `None` off the open region.
    pub revision: Option<RevisionNumber>,
    /// Where the pinned revision forks from the canonical ref (open
    /// nodes); `None` off the open region.
    pub fork_sha: Option<Sha>,
    /// The value the change carries for the grouping key (open nodes of
    /// a grouped graph); `None` for a change without the key, and off the
    /// open region.
    pub group: Option<String>,
}
