//! Assembles the change graph from the two primitive reads.
//!
//! The reads are the repo's change folds (`GET /api/changes`) and the
//! canonical ref's merged history (`GET /api/history`). Runs only in the
//! browser — the server serves the parts, never the whole.
//!
//! Kept `cfg`-free so the host build compiles the same code wasm32 does and
//! the `test-nit-wasm` flake check covers it natively.

use std::collections::{HashMap, HashSet};

use nit_types::domain::Sha;
use nit_types::domain::{ChangeProjection, RevisionProjection};
use nit_types::domain::{ChangeStatus, GraphSection};
use nit_types::graph::{self, ChangeGraph, GraphNode, RepoHistory};

/// One node of the graph's open region.
///
/// An active change pinned at the revision its tip walked.
#[derive(Clone, Copy)]
struct OpenNode<'a> {
    change: &'a ChangeProjection,
    revision: &'a RevisionProjection,
}

/// Commit-sha → the change and revision that produced it.
type ShaIndex<'a> = HashMap<&'a Sha, OpenNode<'a>>;

fn sha_index(changes: &[ChangeProjection]) -> ShaIndex<'_> {
    changes
        .iter()
        .flat_map(|change| {
            change
                .revisions
                .iter()
                .map(move |revision| (&revision.commit_sha, OpenNode { change, revision }))
        })
        .collect()
}

/// Tip commit-shas of the non-terminal changes, sorted.
///
/// A tip is a change's latest-revision sha that no revision records as a
/// `parent_sha`. A superseded revision is never a tip — only the latest
/// revision is a candidate. A merged change is on the canonical ref and an
/// abandoned change is dead, so neither is a tip.
fn tips(changes: &[ChangeProjection]) -> Vec<&Sha> {
    let parents: HashSet<&Sha> = changes
        .iter()
        .flat_map(|c| c.revisions.iter().map(|r| &r.parent_sha))
        .collect();
    let mut tips: Vec<&Sha> = changes
        .iter()
        .filter(|c| !c.is_terminal())
        .filter_map(ChangeProjection::latest_revision)
        .map(|r| &r.commit_sha)
        .filter(|sha| !parents.contains(sha))
        .collect();
    tips.sort();
    tips
}

/// Walks a tip commit-sha back to the canonical ref.
///
/// Follows each revision's recorded `parent_sha`, returning the path
/// oldest-first. The walk stops at the branch: the recorded fork
/// (`parent_sha == fork_sha`), or the first parent that has since
/// merged — so a partially-merged stack walks to its open members
/// alone. **Total**: an unresolved parent (below the merge-base, or a
/// torn push) truncates the path, never errors.
fn path_from_tip<'a>(index: &ShaIndex<'a>, tip: &Sha) -> Vec<OpenNode<'a>> {
    let mut path = Vec::new();
    let mut sha = tip;
    let mut seen = HashSet::new();
    while let Some(node) = index.get(sha) {
        if !seen.insert(sha) {
            break; // cycle guard against bad data
        }
        path.push(*node);
        let parent = &node.revision.parent_sha;
        let parent_merged = index.get(parent).is_some_and(|p| p.change.is_merged());
        if *parent == node.revision.fork_sha || parent_merged {
            break;
        }
        sha = parent;
    }
    path.reverse();
    path
}

/// The graph's **open region**: active tips walked back to their forks.
///
/// Unioned and **deduplicated by commit-sha** — a change shared by two
/// tips appears once, while one change live at two revisions stays two
/// nodes (two shas). In tip-walk order, which seeds the graph's row-order
/// tie-break.
fn open_nodes(changes: &[ChangeProjection]) -> Vec<OpenNode<'_>> {
    let index = sha_index(changes);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tip in tips(changes) {
        for node in path_from_tip(&index, tip) {
            if seen.insert(&node.revision.commit_sha) {
                out.push(node);
            }
        }
    }
    out
}

/// Assembles the canonical-ref-centered DAG from a repo's changes and its
/// merged history.
///
/// The history's HEAD anchor and merged window sit below, every active
/// change ascends above (each tip walked back to its fork, unioned and
/// deduplicated by commit-sha). Nodes are returned in topological row order
/// — the open region ordered among itself (children before parents), the
/// HEAD anchor and its history keeping the canonical-walk order below it. A
/// single global topo would let HEAD — a tip when nothing is built on it —
/// float to the top, which is wrong whenever the whole stack forks behind
/// HEAD.
///
/// `group_by` names the tag key to group the open region on. [`ChangeGraph`]
/// states the order that a grouped graph guarantees.
#[must_use]
pub fn assemble(
    changes: &[ChangeProjection],
    history: &RepoHistory,
    group_by: Option<&str>,
) -> ChangeGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let shas: HashSet<&str> = history.commits.iter().map(|h| h.sha.as_str()).collect();

    // Open region: active changes, in ascending (tip-walk) order.
    for OpenNode { change, revision } in open_nodes(changes) {
        if shas.contains(revision.commit_sha.as_str()) {
            continue; // already placed (an anchor/history sha)
        }
        let group = group_by
            .and_then(|key| change.tags.get(key))
            .map(str::to_string);
        nodes.push(GraphNode {
            commit_sha: revision.commit_sha.clone(),
            section: GraphSection::Open,
            subject: change.subject_at(revision.number),
            status: change.status_at(revision.number),
            parents: vec![revision.parent_sha.clone()],
            change_number: Some(change.id),
            change_id: Some(change.change_id.clone()),
            revision: Some(revision.number),
            fork_sha: Some(revision.fork_sha.clone()),
            group,
        });
    }

    // An open stack's root keeps its real fork (`fork_sha`): the client draws a
    // "behind" edge to it when it is a visible history node, or dangles it into
    // the "earlier history hidden" marker when the fork predates the window.

    let pairs: Vec<(Sha, Vec<Sha>)> = nodes
        .iter()
        .map(|n| (n.commit_sha.clone(), n.parents.clone()))
        .collect();
    let mut order = graph::row_order(&pairs);
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

    ChangeGraph {
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
    use nit_types::domain::Lifecycle;
    use nit_types::domain::Sha;
    use nit_types::domain::{ChangeNumber, RevisionNumber};
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

    /// A node's `(change number, revision number)` pairs, oldest first.
    fn walk(changes: &[ChangeProjection], tip: &str) -> Vec<(u64, u64)> {
        path_from_tip(&sha_index(changes), &sha(tip))
            .iter()
            .map(|n| (n.change.id.get(), n.revision.number.get()))
            .collect()
    }

    /// push1 `m → A → B → C` and push2 `m → D → B' → E`: B is one change at two
    /// revisions, surfaced as two tips.
    #[test]
    fn b_under_two_tips() {
        let ca = change(10, "Ia", vec![revision(0, "A", "m", "m")]);
        let cb = change(
            11,
            "Ib",
            vec![revision(0, "B", "A", "m"), revision(1, "Bp", "D", "m")],
        );
        let cc = change(12, "Ic", vec![revision(0, "C", "B", "m")]);
        let cd = change(13, "Id", vec![revision(0, "D", "m", "m")]);
        let ce = change(14, "Ie", vec![revision(0, "E", "Bp", "m")]);
        let changes = vec![ca, cb, cc, cd, ce];

        assert_eq!(tips(&changes), vec![&sha("C"), &sha("E")]);
        assert_eq!(walk(&changes, "C"), vec![(10, 0), (11, 0), (12, 0)]);
        assert_eq!(walk(&changes, "E"), vec![(13, 0), (11, 1), (14, 0)]);

        // Open nodes dedupe by sha, so B@rev0 (sha "B") and B@rev1 (sha "Bp")
        // are two nodes — different commits, different parents.
        let mut shas: Vec<&Sha> = open_nodes(&changes)
            .iter()
            .map(|n| &n.revision.commit_sha)
            .collect();
        shas.sort();
        let mut want = ["A", "B", "Bp", "C", "D", "E"].map(sha);
        want.sort();
        assert_eq!(shas, want.iter().collect::<Vec<_>>());
    }

    #[test]
    fn walk_stops_at_a_merged_ancestor() {
        // A → B forked from "m"; A has since merged. The walk stops at
        // the canonical ref, so B's path is the open member alone — the
        // merged ancestor sits below the branch now.
        let mut a = change(1, "Ia", vec![revision(0, "A", "m", "m")]);
        a.lifecycle = Lifecycle::Merged;
        let b = change(2, "Ib", vec![revision(0, "B", "A", "m")]);
        let changes = vec![a, b];

        assert_eq!(walk(&changes, "B"), vec![(2, 0)]);
        let open: Vec<u64> = open_nodes(&changes)
            .iter()
            .map(|n| n.change.id.get())
            .collect();
        assert_eq!(open, vec![2], "no merged node leaks into the open region");
    }

    #[test]
    fn prefix_branch_is_subsumed() {
        let a = change(1, "Ia", vec![revision(0, "A", "m", "m")]);
        let b = change(2, "Ib", vec![revision(0, "B", "A", "m")]);
        let c = change(3, "Ic", vec![revision(0, "C", "B", "m")]);
        assert_eq!(tips(&[a, b, c]), vec![&sha("C")]);
    }

    #[test]
    fn open_fork_behind_head_orders_above_anchor_and_keeps_its_base() {
        let topic = change(1, "It", vec![revision(0, "T", "c1", "c1")]);
        let changes = vec![topic];
        let history = RepoHistory {
            commits: vec![
                commit("c3", &["c2"]),
                commit("c2", &["c1"]),
                commit("c1", &[]),
            ],
            truncated: false,
        };

        let g = assemble(&changes, &history, None);
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
        let changes = vec![a, b];
        let merged = HistoryCommit {
            change_number: Some(ChangeNumber::new(9)),
            change_id: Some(change_id("Iland")),
            ..commit("g1", &["g2"])
        };
        let history = RepoHistory {
            commits: vec![commit("h", &["g1"]), merged, commit("g2", &[])],
            truncated: true,
        };

        let g = assemble(&changes, &history, None);
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

    // Two stacks off HEAD, each one session, and a second-session change
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
        let changes = vec![p1, p2, q, r1, r2];
        let history = RepoHistory {
            commits: vec![commit("h", &[])],
            truncated: false,
        };
        let shas = |g: &ChangeGraph| {
            g.nodes
                .iter()
                .map(|n| n.commit_sha.clone())
                .collect::<Vec<_>>()
        };

        let plain = assemble(&changes, &history, None);
        assert_eq!(shas(&plain), ["Q", "R2", "P2", "R1", "P1", "h"].map(sha));

        let grouped = assemble(&changes, &history, Some("s"));
        assert_eq!(shas(&grouped), ["Q", "R2", "R1", "P2", "P1", "h"].map(sha));
        let groups: Vec<Option<&str>> = grouped.nodes.iter().map(|n| n.group.as_deref()).collect();
        assert_eq!(
            groups,
            [Some("2"), Some("2"), Some("2"), Some("1"), Some("1"), None]
        );

        let other = assemble(&changes, &history, Some("absent"));
        assert_eq!(shas(&other), shas(&plain));
        assert!(other.nodes.iter().all(|n| n.group.is_none()));
    }
}
