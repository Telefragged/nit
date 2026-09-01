//! Assembles the change graph from the two primitive reads.
//!
//! The reads are the repo's change folds (`GET /api/changes`) and the
//! canonical ref's merged history (`GET /api/history`). Runs only in the
//! browser — the server serves the parts, never the whole.
//!
//! Kept `cfg`-free so the host build compiles the same code wasm32 does and
//! the `test-nit-wasm` flake check covers it natively.

use std::collections::{HashMap, HashSet};

use nit_types::chain::{self, RepoView};
use nit_types::domain::Sha;
use nit_types::domain::{ChangeStatus, GraphSection};
use nit_types::graph::{GraphNode, RepoGraph, RepoHistory};

/// Assembles the canonical-ref-centered DAG from a repo view and its merged history.
///
/// The history's HEAD anchor and merged window sit below, every active
/// change ascends above (the same derivation as a chain, unioned and
/// deduplicated by commit-sha). Nodes are returned in topological row order
/// — the open region ordered among itself (children before parents), the
/// HEAD anchor and its history keeping the canonical-walk order below it. A
/// single global topo would let HEAD — a tip when nothing is built on it —
/// float to the top, which is wrong whenever the whole chain forks behind
/// HEAD.
///
/// `group_by` names the tag key to group the open region on. [`RepoGraph`]
/// states the order that a grouped graph guarantees.
#[must_use]
pub fn assemble(view: &RepoView, history: &RepoHistory, group_by: Option<&str>) -> RepoGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let shas: HashSet<&str> = history.commits.iter().map(|h| h.sha.as_str()).collect();

    // Open region: active changes, in ascending (tip-walk) order.
    for node in view.open_nodes() {
        if shas.contains(node.commit_sha.as_str()) {
            continue; // already placed (an anchor/history sha)
        }
        let Some(change) = view.change(node.change_number) else {
            continue;
        };
        let group = group_by
            .and_then(|key| change.tags.get(key))
            .map(str::to_string);
        nodes.push(GraphNode {
            commit_sha: node.commit_sha,
            section: GraphSection::Open,
            subject: change.subject_at(node.revision),
            status: change.status_at(node.revision),
            parents: node.parent_sha.into_iter().collect(),
            change_number: Some(change.id),
            change_id: Some(change.change_id.clone()),
            revision: Some(node.revision),
            fork_sha: node.fork_sha,
            group,
        });
    }

    // An open chain's root keeps its real fork (`fork_sha`): the client draws a
    // "behind" edge to it when it is a visible history node, or dangles it into
    // the "earlier history hidden" marker when the fork predates the window.

    let pairs: Vec<(Sha, Vec<Sha>)> = nodes
        .iter()
        .map(|n| (n.commit_sha.clone(), n.parents.clone()))
        .collect();
    let mut order = chain::graph_row_order(&pairs);
    if group_by.is_some() {
        let groups: HashMap<&Sha, Option<&str>> = nodes
            .iter()
            .map(|n| (&n.commit_sha, n.group.as_deref()))
            .collect();
        order = grouped_order(&order, &pairs, &groups);
    }
    let pos: HashMap<Sha, usize> = order
        .into_iter()
        .enumerate()
        .map(|(i, sha)| (sha, i))
        .collect();
    nodes.sort_by_key(|n| pos.get(&n.commit_sha).copied().unwrap_or(usize::MAX));

    // The HEAD anchor + history keep the canonical-walk order below the open
    // region.
    nodes.extend(
        history
            .commits
            .iter()
            .enumerate()
            .map(|(depth, h)| GraphNode {
                commit_sha: h.sha.clone(),
                section: if depth == 0 {
                    GraphSection::Head
                } else {
                    GraphSection::History
                },
                subject: h.subject.clone(),
                status: ChangeStatus::Merged,
                parents: h.parents.clone(),
                change_number: h.change_number,
                change_id: h.change_id.clone(),
                revision: None,
                fork_sha: None,
                group: None,
            }),
    );

    RepoGraph {
        history_truncated: history.truncated,
        nodes,
    }
}

/// Reorders a topological `order` so that each group's nodes sit together.
///
/// A group is the set of nodes with one value in `groups`. A group ranks
/// by its first node in `order`. The result stays topological: this
/// function places a node only after it places every child of that node.
/// It continues the current group while that group has such a node.
/// Otherwise it starts the lowest-ranked group that has one, so a group
/// splits only where a node of another group sits between two of its own.
fn grouped_order(
    order: &[Sha],
    pairs: &[(Sha, Vec<Sha>)],
    groups: &HashMap<&Sha, Option<&str>>,
) -> Vec<Sha> {
    let mut children: HashMap<&Sha, Vec<&Sha>> = HashMap::new();
    for (sha, parents) in pairs {
        for parent in parents {
            children.entry(parent).or_default().push(sha);
        }
    }
    let mut rank_of: HashMap<Option<&str>, usize> = HashMap::new();
    for sha in order {
        let next = rank_of.len();
        rank_of.entry(groups[sha]).or_insert(next);
    }
    let rank = |sha: &Sha| rank_of[&groups[sha]];

    let mut placed: HashSet<&Sha> = HashSet::new();
    let mut out: Vec<Sha> = Vec::with_capacity(order.len());
    let mut current = None;
    while out.len() < order.len() {
        let ready = |sha: &&Sha| {
            !placed.contains(sha)
                && children
                    .get(sha)
                    .is_none_or(|kids| kids.iter().all(|kid| placed.contains(kid)))
        };
        // The first minimum wins, so the current group's earliest ready node
        // comes before any other group's.
        let next = order
            .iter()
            .filter(ready)
            .min_by_key(|sha| (Some(rank(sha)) != current, rank(sha)));
        let Some(next) = next else {
            break; // cycle guard against bad data
        };
        current = Some(rank(next));
        placed.insert(next);
        out.push(next.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nit_types::domain::Sha;
    use nit_types::domain::{ChangeNumber, RevisionNumber};
    use nit_types::domain::{ChangeProjection, RevisionProjection};
    use nit_types::graph::HistoryCommit;
    use nit_types::testing::{change_id, sha};

    fn revision(number: u64, name: &str, parent: &str, base: &str) -> RevisionProjection {
        RevisionProjection {
            number: RevisionNumber::new(number),
            commit_sha: sha(name),
            parent_sha: sha(parent),
            fork_sha: sha(base),
            message: format!("subject {name}"),
            resets_status: true,
            created_at: "t0".to_string(),
        }
    }

    fn change(number: u64, key: &str, revs: Vec<RevisionProjection>) -> ChangeProjection {
        let mut c = ChangeProjection::new(ChangeNumber::new(number), 1, change_id(key));
        c.revisions = revs;
        c
    }

    fn commit(name: &str, parents: &[&str]) -> HistoryCommit {
        HistoryCommit {
            sha: sha(name),
            parents: parents.iter().map(|p| sha(p)).collect(),
            subject: format!("main {name}"),
            change_number: None,
            change_id: None,
        }
    }

    #[test]
    fn open_fork_behind_head_orders_above_anchor_and_keeps_its_base() {
        let topic = change(1, "It", vec![revision(0, "T", "c1", "c1")]);
        let view = RepoView::new(vec![topic]);
        let history = RepoHistory {
            commits: vec![
                commit("c3", &["c2"]),
                commit("c2", &["c1"]),
                commit("c1", &[]),
            ],
            truncated: false,
        };

        let g = assemble(&view, &history, None);
        let row = |name: &str| {
            g.nodes
                .iter()
                .position(|n| n.commit_sha == sha(name))
                .unwrap_or_else(|| panic!("no node {name}"))
        };
        assert_eq!(g.nodes[row("c3")].section, GraphSection::Head);
        assert_eq!(g.nodes[row("T")].section, GraphSection::Open);
        assert!(
            row("T") < row("c3"),
            "open fork must order above the HEAD anchor: {:?}",
            g.nodes.iter().map(|n| &n.commit_sha).collect::<Vec<_>>()
        );
        assert_eq!(
            g.nodes[row("T")].parents,
            vec![sha("c1")],
            "topic keeps its real fork base, never re-rooted onto HEAD"
        );
        assert_eq!(g.nodes[row("T")].fork_sha, Some(sha("c1")));
        assert_eq!(g.nodes[row("c3")].fork_sha, None);
    }

    // The enriched history rides through untouched, an anchor-sha open node
    // dedupes away, and truncation is the history's flag.
    #[test]
    fn history_enrichment_and_truncation_ride_through() {
        let a = change(1, "Ia", vec![revision(0, "A", "h", "h")]);
        let b = change(2, "Ib", vec![revision(0, "B", "A", "h")]);
        let view = RepoView::new(vec![a, b]);
        let merged = HistoryCommit {
            change_number: Some(ChangeNumber::new(9)),
            change_id: Some(change_id("Iland")),
            ..commit("g1", &["g2"])
        };
        let history = RepoHistory {
            commits: vec![commit("h", &["g1"]), merged, commit("g2", &[])],
            truncated: true,
        };

        let g = assemble(&view, &history, None);
        assert!(g.history_truncated);
        let shas: Vec<&Sha> = g.nodes.iter().map(|n| &n.commit_sha).collect();
        // Children ascend: the tip B sits above its parent A, both above HEAD.
        assert_eq!(
            shas,
            ["B", "A", "h", "g1", "g2"]
                .map(sha)
                .iter()
                .collect::<Vec<_>>()
        );
        let g1 = g
            .nodes
            .iter()
            .find(|n| n.commit_sha == sha("g1"))
            .expect("g1");
        assert_eq!(g1.change_number, Some(ChangeNumber::new(9)));
        assert_eq!(g1.change_id, Some(change_id("Iland")));
    }

    fn tagged(mut change: ChangeProjection, key: &str, value: &str) -> ChangeProjection {
        change.tags = nit_types::testing::tags(&[(key, value)]);
        change
    }

    // Two chains off HEAD, each one session, and a second-session change
    // on top of the first. Ungrouped, the topological order interleaves
    // the sessions by rank. Grouped, each session is one run: the change
    // stacked across sessions does not split its own group's run.
    #[test]
    fn grouping_runs_each_tag_value_together() {
        let p1 = tagged(
            change(1, "Ip1", vec![revision(0, "P1", "h", "h")]),
            "s",
            "1",
        );
        let p2 = tagged(
            change(2, "Ip2", vec![revision(0, "P2", "P1", "h")]),
            "s",
            "1",
        );
        let q = tagged(change(3, "Iq", vec![revision(0, "Q", "P2", "h")]), "s", "2");
        let r1 = tagged(
            change(4, "Ir1", vec![revision(0, "R1", "h", "h")]),
            "s",
            "2",
        );
        let r2 = tagged(
            change(5, "Ir2", vec![revision(0, "R2", "R1", "h")]),
            "s",
            "2",
        );
        let view = RepoView::new(vec![p1, p2, q, r1, r2]);
        let history = RepoHistory {
            commits: vec![commit("h", &[])],
            truncated: false,
        };
        let shas = |g: &RepoGraph| {
            g.nodes
                .iter()
                .map(|n| n.commit_sha.clone())
                .collect::<Vec<_>>()
        };

        let plain = assemble(&view, &history, None);
        assert_eq!(shas(&plain), ["Q", "R2", "P2", "R1", "P1", "h"].map(sha));

        let grouped = assemble(&view, &history, Some("s"));
        assert_eq!(shas(&grouped), ["Q", "R2", "R1", "P2", "P1", "h"].map(sha));
        let groups: Vec<Option<&str>> = grouped.nodes.iter().map(|n| n.group.as_deref()).collect();
        assert_eq!(
            groups,
            [Some("2"), Some("2"), Some("2"), Some("1"), Some("1"), None]
        );

        let other = assemble(&view, &history, Some("absent"));
        assert_eq!(shas(&other), shas(&plain));
        assert!(other.nodes.iter().all(|n| n.group.is_none()));
    }
}
