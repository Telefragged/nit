//! The spine-centered change graph.

use serde::{Deserialize, Serialize};

use crate::domain::{ChangeStatus, GraphSection};

/// One commit of the canonical branch's merged history.
///
/// Walked from the tracked ref's HEAD down (`GET /api/history?repo={id}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HistoryCommit {
    /// Full 40-hex commit-sha.
    pub sha: String,
    /// Parent commit-shas; more than one is a merge.
    pub parents: Vec<String>,
    pub subject: String,
    /// The landed change this commit carries, matched by its `Change-Id:`
    /// trailer. Coupled with `change_key`: a commit whose trailer names no
    /// known change (a merge, a pre-nit commit, a foreign trailer) reports
    /// both as `None`, never an orphan key.
    pub change_id: Option<u64>,
    pub change_key: Option<String>,
}

/// A window of the canonical branch's merged history (`GET /api/history`).
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

/// One repo's change graph: a commit-sha-keyed DAG over the canonical branch.
///
/// Not a response body — the browser assembles it (`crates/nit-wasm`) from
/// the two primitive reads, `GET /api/changes` and `GET /api/history`; the
/// shape lives here because it crosses the wasm↔JS boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoGraph {
    /// The canonical branch has merged commits below the displayed window — the
    /// client shows an "earlier history hidden" marker and dangles deep forks
    /// to it.
    pub history_truncated: bool,
    /// Row order, top → bottom: open (top) → head → history (bottom).
    ///
    /// A topological order in which every node precedes its parents.
    pub nodes: Vec<GraphNode>,
}

/// One node of the change graph, keyed by its `commit_sha`.
///
/// Edges are its `parents` (an edge is drawn to each that is in the node
/// set; `len > 1` is a merge).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GraphNode {
    /// The node's stable id — a full 40-hex commit-sha; the client truncates.
    pub commit_sha: String,
    pub section: GraphSection,
    pub subject: String,
    /// `ChangeStatus` at the pinned revision; head/history read as merged.
    ///
    /// The client styles by `section`.
    pub status: ChangeStatus,
    /// Parent commit-shas; an edge is drawn to each that is in the node set.
    pub parents: Vec<String>,
    /// The backing change, or `None` for a bare git commit (merge / pre-nit).
    pub change_id: Option<u64>,
    pub change_key: Option<String>,
    /// The pinned revision (open nodes); `None` off the open region.
    pub revision: Option<u64>,
}
