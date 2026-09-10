//! The change graph, centered on the canonical ref.

use std::collections::HashMap;

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
    /// HEAD-first, and every commit precedes its parents. Each commit's
    /// `parents` carry the edges.
    pub commits: Vec<HistoryCommit>,
    /// The branch has more merged commits below the window.
    pub truncated: bool,
}

/// A change graph: a commit-sha-keyed DAG over the canonical ref.
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
pub struct ChangeGraph {
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

/// Row order for the change graph: every node precedes its parents.
///
/// A topological order — children ascend, parents descend, so the
/// canonical HEAD sits between its open descendants and its merged
/// ancestors. `nodes` is `(commit_sha, in-set parent shas)` in a stable
/// input order; the returned shas are top → bottom.
///
/// A node's rank is `0` for a tip, else `1 + max(child rank)`; nodes sort by
/// `(rank, input order)`. Rank places every parent strictly below its
/// children and groups a fan-out's branches adjacently; the input-order
/// tie-break keeps it deterministic.
#[must_use]
pub fn row_order(nodes: &[(Sha, Vec<Sha>)]) -> Vec<Sha> {
    fn rank(
        i: usize,
        children: &[Vec<usize>],
        memo: &mut [Option<u64>],
        on_stack: &mut [bool],
    ) -> u64 {
        if let Some(r) = memo[i] {
            return r;
        }
        if on_stack[i] {
            return 0; // cycle guard against bad data
        }
        on_stack[i] = true;
        let r = children[i]
            .iter()
            .map(|&c| rank(c, children, memo, on_stack))
            .max()
            .map_or(0, |m| m + 1);
        on_stack[i] = false;
        memo[i] = Some(r);
        r
    }

    let index: HashMap<&Sha, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (sha, _))| (sha, i))
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, (_, parents)) in nodes.iter().enumerate() {
        for p in parents {
            if let Some(&pi) = index.get(p) {
                children[pi].push(i);
            }
        }
    }

    let mut memo = vec![None; nodes.len()];
    let mut on_stack = vec![false; nodes.len()];
    let ranks: Vec<u64> = (0..nodes.len())
        .map(|i| rank(i, &children, &mut memo, &mut on_stack))
        .collect();
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| (ranks[i], i));
    order.into_iter().map(|i| nodes[i].0.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sha;

    #[test]
    fn row_order_is_topological_children_before_parents() {
        // The change-graph mock topology: two open tips (A1, A2) fanning from
        // A3 → A4 → HEAD, then merged history H → G1 → G2(merge of G3,G4) → G5.
        let pairs = vec![
            (sha("A1"), vec![sha("A3")]),
            (sha("A2"), vec![sha("A3")]),
            (sha("A3"), vec![sha("A4")]),
            (sha("A4"), vec![sha("H")]),
            (sha("H"), vec![sha("G1")]),
            (sha("G1"), vec![sha("G2")]),
            (sha("G2"), vec![sha("G3"), sha("G4")]),
            (sha("G3"), vec![sha("G5")]),
            (sha("G4"), vec![sha("G5")]),
            (sha("G5"), vec![]),
        ];
        assert_eq!(
            row_order(&pairs),
            ["A1", "A2", "A3", "A4", "H", "G1", "G2", "G3", "G4", "G5"].map(sha)
        );
        // None < Some keeps the comparison honest if a sha is ever missing.
        let order = row_order(&pairs);
        let pos = |s: &Sha| order.iter().position(|x| x == s);
        for (child, parents) in &pairs {
            for p in parents {
                assert!(pos(child) < pos(p), "{child} should precede parent {p}");
            }
        }
    }
}
