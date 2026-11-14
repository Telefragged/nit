//! Assembles a change graph from change folds.
//!
//! The repo graph joins the repo's change folds (`GET /api/changes`) to
//! the canonical ref's merged history (`GET /api/history`). The tag graph
//! is the folds alone, at their latest revisions. Both run only in the
//! browser — the server serves the parts, never the whole.
//!
//! Kept `cfg`-free so the host build compiles the same code wasm32 does and
//! the `test-nit-wasm` flake check covers it natively.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use nit_types::domain::Sha;
use nit_types::domain::{ChangeProjection, RevisionProjection};
use nit_types::domain::{ChangeStatus, GraphSection};
use nit_types::graph::{self, ChangeGraph, GraphNode, RepoHistory};

/// One node of a graph's open region: a change pinned at one revision.
#[derive(Clone, Copy)]
struct OpenNode<'a> {
    change: &'a ChangeProjection,
    revision: &'a RevisionProjection,
}

impl OpenNode<'_> {
    /// The node as the graph carries it, `group` its value for the
    /// grouping key.
    fn graph_node(self, group: Option<String>) -> GraphNode {
        let OpenNode { change, revision } = self;
        GraphNode {
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
        }
    }
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
    let shas: HashSet<&str> = history.commits.iter().map(|h| h.sha.as_str()).collect();

    // Open region: active changes, in ascending (tip-walk) order, less the
    // ones already placed as an anchor or history sha.
    let open: Vec<OpenNode<'_>> = open_nodes(changes)
        .into_iter()
        .filter(|n| !shas.contains(n.revision.commit_sha.as_str()))
        .collect();
    let groups: Vec<Option<&str>> = open
        .iter()
        .map(|n| group_by.and_then(|key| n.change.tags.get(key)))
        .collect();
    let nodes = open
        .iter()
        .zip(&groups)
        .map(|(n, group)| n.graph_node(group.map(str::to_string)))
        .collect();
    let mut nodes = ordered(nodes, &groups, |_| ());

    // An open stack's root keeps its real fork (`fork_sha`): the client draws a
    // "behind" edge to it when it is a visible history node, or dangles it into
    // the "earlier history hidden" marker when the fork predates the window.

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

/// Sorts `nodes` into row order, each group's nodes adjacent.
///
/// `groups[i]` is the group of `nodes[i]`. The base is a topological
/// order, children before parents. A group ranks by `priority` first and
/// by its first node in that order second, and the groups run in rank
/// order where the topological order allows.
fn ordered<G: Hash + Eq + Copy, P: Ord + Copy>(
    mut nodes: Vec<GraphNode>,
    groups: &[G],
    priority: impl Fn(G) -> P,
) -> Vec<GraphNode> {
    let pairs: Vec<(Sha, Vec<Sha>)> = nodes
        .iter()
        .map(|n| (n.commit_sha.clone(), n.parents.clone()))
        .collect();
    let group_of: HashMap<&Sha, G> = nodes
        .iter()
        .zip(groups)
        .map(|(n, group)| (&n.commit_sha, *group))
        .collect();
    let order = graph::row_order(&pairs);
    let mut rank_of: HashMap<G, (P, usize)> = HashMap::new();
    for sha in &order {
        let next = rank_of.len();
        rank_of
            .entry(group_of[sha])
            .or_insert_with(|| (priority(group_of[sha]), next));
    }
    let order = grouped_order(&order, &pairs, |sha| rank_of[&group_of[sha]]);
    let pos: HashMap<Sha, usize> = order
        .into_iter()
        .enumerate()
        .map(|(i, sha)| (sha, i))
        .collect();
    nodes.sort_by_key(|n| pos.get(&n.commit_sha).copied().unwrap_or(usize::MAX));
    nodes
}

/// Reorders a topological `order` so that each group's nodes sit together.
///
/// `rank` names each node's group by the group's rank. The result stays
/// topological: this function places a node only after it places every
/// child of that node. It continues the current group while that group
/// has such a node. Otherwise it starts the lowest-ranked group that has
/// one, so a group splits only where a node of another group sits
/// between two of its own.
fn grouped_order<K: Ord + Copy>(
    order: &[Sha],
    pairs: &[(Sha, Vec<Sha>)],
    rank: impl Fn(&Sha) -> K,
) -> Vec<Sha> {
    let mut children: HashMap<&Sha, Vec<&Sha>> = HashMap::new();
    for (sha, parents) in pairs {
        for parent in parents {
            children.entry(parent).or_default().push(sha);
        }
    }

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

/// The root of a union-find set, with path halving.
fn root(link: &mut [usize], i: usize) -> usize {
    let mut i = i;
    while link[i] != i {
        link[i] = link[link[i]];
        i = link[i];
    }
    i
}

/// Assembles the tag graph: `changes` at their latest revisions.
///
/// One node per change, pinned at its latest revision whatever its
/// lifecycle. A node's edge goes to the node whose commit is its parent.
/// So a change whose parent is not another member's latest revision is a
/// root, and a merged or abandoned change is a root of its own unless a
/// member still sits on it. Rows are topological, and the nodes of one
/// connected component are adjacent. A component with an active change
/// precedes a component whose every change is merged or abandoned. Among
/// components, and among the tips of one, the order of `changes` breaks
/// the tie.
#[must_use]
pub fn assemble_tag(changes: &[ChangeProjection]) -> ChangeGraph {
    let latest: Vec<OpenNode<'_>> = changes
        .iter()
        .filter_map(|change| {
            let revision = change.latest_revision()?;
            Some(OpenNode { change, revision })
        })
        .collect();
    let index: HashMap<&Sha, usize> = latest
        .iter()
        .enumerate()
        .map(|(i, n)| (&n.revision.commit_sha, i))
        .collect();

    // Connected components over the in-set parent edges.
    let mut link: Vec<usize> = (0..latest.len()).collect();
    for (i, n) in latest.iter().enumerate() {
        if let Some(&parent) = index.get(&n.revision.parent_sha) {
            let (a, b) = (root(&mut link, i), root(&mut link, parent));
            link[a] = b;
        }
    }
    let component: Vec<usize> = (0..latest.len()).map(|i| root(&mut link, i)).collect();
    let active: HashSet<usize> = latest
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.change.is_terminal())
        .map(|(i, _)| component[i])
        .collect();

    let nodes = latest.iter().map(|n| n.graph_node(None)).collect();
    ChangeGraph {
        history_truncated: false,
        nodes: ordered(nodes, &component, |c| !active.contains(&c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nit_types::domain::Lifecycle;
    use nit_types::domain::{ChangeId, Sha, Tag, Tags};
    use nit_types::domain::{ChangeNumber, RevisionNumber};
    use nit_types::graph::HistoryCommit;

    fn revision(number: u64, commit: &Sha, parent: &Sha, base: &Sha) -> RevisionProjection {
        RevisionProjection {
            number: RevisionNumber::new(number),
            commit_sha: commit.clone(),
            parent_sha: parent.clone(),
            fork_sha: base.clone(),
            message: format!("subject {commit}"),
            resets_status: true,
            created_at: "t0".to_string(),
            attachments: vec![],
        }
    }

    fn change(number: u64, id: &ChangeId, revs: Vec<RevisionProjection>) -> ChangeProjection {
        let mut c = ChangeProjection::new(ChangeNumber::new(number), 1, id.clone());
        c.revisions = revs;
        c
    }

    fn commit(sha: &Sha, parents: &[&Sha]) -> HistoryCommit {
        HistoryCommit {
            sha: sha.clone(),
            parents: parents.iter().map(|&p| p.clone()).collect(),
            subject: format!("main {sha}"),
            change_number: None,
            change_id: None,
        }
    }

    /// A node's `(change number, revision number)` pairs, oldest first.
    fn walk(changes: &[ChangeProjection], tip: &Sha) -> Vec<(u64, u64)> {
        path_from_tip(&sha_index(changes), tip)
            .iter()
            .map(|n| (n.change.id.get(), n.revision.number.get()))
            .collect()
    }

    /// push1 `m → A → B → C` and push2 `m → D → B' → E`: B is one change at two
    /// revisions, surfaced as two tips.
    #[test]
    fn b_under_two_tips() {
        let sha_m = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let sha_a = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let sha_b = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let bp = Sha::new("4444444444444444444444444444444444444444").expect("valid sha");
        let sha_c = Sha::new("5555555555555555555555555555555555555555").expect("valid sha");
        let sha_d = Sha::new("6666666666666666666666666666666666666666").expect("valid sha");
        let sha_e = Sha::new("7777777777777777777777777777777777777777").expect("valid sha");
        let ida =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let idb =
            ChangeId::new("I2222222222222222222222222222222222222222").expect("valid change id");
        let idc =
            ChangeId::new("I3333333333333333333333333333333333333333").expect("valid change id");
        let idd =
            ChangeId::new("I4444444444444444444444444444444444444444").expect("valid change id");
        let ide =
            ChangeId::new("I5555555555555555555555555555555555555555").expect("valid change id");
        let ca = change(10, &ida, vec![revision(0, &sha_a, &sha_m, &sha_m)]);
        let cb = change(
            11,
            &idb,
            vec![
                revision(0, &sha_b, &sha_a, &sha_m),
                revision(1, &bp, &sha_d, &sha_m),
            ],
        );
        let cc = change(12, &idc, vec![revision(0, &sha_c, &sha_b, &sha_m)]);
        let cd = change(13, &idd, vec![revision(0, &sha_d, &sha_m, &sha_m)]);
        let ce = change(14, &ide, vec![revision(0, &sha_e, &bp, &sha_m)]);
        let changes = vec![ca, cb, cc, cd, ce];

        assert_eq!(tips(&changes), vec![&sha_c, &sha_e]);
        assert_eq!(walk(&changes, &sha_c), vec![(10, 0), (11, 0), (12, 0)]);
        assert_eq!(walk(&changes, &sha_e), vec![(13, 0), (11, 1), (14, 0)]);

        // Open nodes dedupe by sha, so B@rev0 (sha `b`) and B@rev1 (sha `bp`)
        // are two nodes — different commits, different parents.
        let mut shas: Vec<&Sha> = open_nodes(&changes)
            .iter()
            .map(|n| &n.revision.commit_sha)
            .collect();
        shas.sort();
        let mut want = [&sha_a, &sha_b, &bp, &sha_c, &sha_d, &sha_e];
        want.sort();
        assert_eq!(shas, want);
    }

    #[test]
    fn walk_stops_at_a_merged_ancestor() {
        let sha_m = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let sha_a = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let sha_b = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let ida =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let idb =
            ChangeId::new("I2222222222222222222222222222222222222222").expect("valid change id");
        // A → B forked from "m"; A has since merged. The walk stops at
        // the canonical ref, so B's path is the open member alone — the
        // merged ancestor sits below the branch now.
        let mut ca = change(1, &ida, vec![revision(0, &sha_a, &sha_m, &sha_m)]);
        ca.lifecycle = Lifecycle::Merged;
        let cb = change(2, &idb, vec![revision(0, &sha_b, &sha_a, &sha_m)]);
        let changes = vec![ca, cb];

        assert_eq!(walk(&changes, &sha_b), vec![(2, 0)]);
        let open: Vec<u64> = open_nodes(&changes)
            .iter()
            .map(|n| n.change.id.get())
            .collect();
        assert_eq!(open, vec![2], "no merged node leaks into the open region");
    }

    #[test]
    fn prefix_branch_is_subsumed() {
        let sha_m = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let sha_a = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let sha_b = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let sha_c = Sha::new("4444444444444444444444444444444444444444").expect("valid sha");
        let ida =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let idb =
            ChangeId::new("I2222222222222222222222222222222222222222").expect("valid change id");
        let idc =
            ChangeId::new("I3333333333333333333333333333333333333333").expect("valid change id");
        let ca = change(1, &ida, vec![revision(0, &sha_a, &sha_m, &sha_m)]);
        let cb = change(2, &idb, vec![revision(0, &sha_b, &sha_a, &sha_m)]);
        let cc = change(3, &idc, vec![revision(0, &sha_c, &sha_b, &sha_m)]);
        assert_eq!(tips(&[ca, cb, cc]), vec![&sha_c]);
    }

    #[test]
    fn open_fork_behind_head_orders_above_anchor_and_keeps_its_base() {
        let sha_t = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let c1 = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let c2 = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let c3 = Sha::new("4444444444444444444444444444444444444444").expect("valid sha");
        let idt =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let topic = change(1, &idt, vec![revision(0, &sha_t, &c1, &c1)]);
        let changes = vec![topic];
        let history = RepoHistory {
            commits: vec![commit(&c3, &[&c2]), commit(&c2, &[&c1]), commit(&c1, &[])],
            truncated: false,
        };

        let g = assemble(&changes, &history, None);
        let row = |sha: &Sha| {
            g.nodes
                .iter()
                .position(|n| &n.commit_sha == sha)
                .unwrap_or_else(|| panic!("no node {sha}"))
        };
        assert_eq!(g.nodes[row(&c3)].section, GraphSection::Head);
        assert_eq!(g.nodes[row(&sha_t)].section, GraphSection::Open);
        assert!(
            row(&sha_t) < row(&c3),
            "open fork must order above the HEAD anchor: {:?}",
            g.nodes.iter().map(|n| &n.commit_sha).collect::<Vec<_>>()
        );
        assert_eq!(
            g.nodes[row(&sha_t)].parents,
            vec![c1.clone()],
            "topic keeps its real fork base, never re-rooted onto HEAD"
        );
        assert_eq!(g.nodes[row(&sha_t)].fork_sha, Some(c1));
        assert_eq!(g.nodes[row(&c3)].fork_sha, None);
    }

    // The enriched history rides through untouched, an anchor-sha open node
    // dedupes away, and truncation is the history's flag.
    #[test]
    fn history_enrichment_and_truncation_ride_through() {
        let sha_a = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let sha_b = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let sha_h = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let g1 = Sha::new("4444444444444444444444444444444444444444").expect("valid sha");
        let g2 = Sha::new("5555555555555555555555555555555555555555").expect("valid sha");
        let ida =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let idb =
            ChangeId::new("I2222222222222222222222222222222222222222").expect("valid change id");
        let idland =
            ChangeId::new("I3333333333333333333333333333333333333333").expect("valid change id");
        let ca = change(1, &ida, vec![revision(0, &sha_a, &sha_h, &sha_h)]);
        let cb = change(2, &idb, vec![revision(0, &sha_b, &sha_a, &sha_h)]);
        let changes = vec![ca, cb];
        let merged = HistoryCommit {
            change_number: Some(ChangeNumber::new(9)),
            change_id: Some(idland.clone()),
            ..commit(&g1, &[&g2])
        };
        let history = RepoHistory {
            commits: vec![commit(&sha_h, &[&g1]), merged, commit(&g2, &[])],
            truncated: true,
        };

        let g = assemble(&changes, &history, None);
        assert!(g.history_truncated);
        let shas: Vec<&Sha> = g.nodes.iter().map(|n| &n.commit_sha).collect();
        // Children ascend: the tip B sits above its parent A, both above HEAD.
        assert_eq!(shas, [&sha_b, &sha_a, &sha_h, &g1, &g2]);
        let node_g1 = g.nodes.iter().find(|n| n.commit_sha == g1).expect("g1");
        assert_eq!(node_g1.change_number, Some(ChangeNumber::new(9)));
        assert_eq!(node_g1.change_id, Some(idland));
    }

    fn tagged(mut change: ChangeProjection, key: &str, value: &str) -> ChangeProjection {
        change.tags = [Tag::new(key, value).expect("valid tag")]
            .into_iter()
            .collect::<Tags>();
        change
    }

    // Two stacks off HEAD, each one session, and a second-session change
    // on top of the first. Ungrouped, the topological order interleaves
    // the sessions by rank. Grouped, each session is one run: the change
    // stacked across sessions does not split its own group's run.
    #[test]
    fn grouping_runs_each_tag_value_together() {
        let sha_h = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let p1 = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let p2 = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let sha_q = Sha::new("4444444444444444444444444444444444444444").expect("valid sha");
        let r1 = Sha::new("5555555555555555555555555555555555555555").expect("valid sha");
        let r2 = Sha::new("6666666666666666666666666666666666666666").expect("valid sha");
        let idp1 =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let idp2 =
            ChangeId::new("I2222222222222222222222222222222222222222").expect("valid change id");
        let idq =
            ChangeId::new("I3333333333333333333333333333333333333333").expect("valid change id");
        let idr1 =
            ChangeId::new("I4444444444444444444444444444444444444444").expect("valid change id");
        let idr2 =
            ChangeId::new("I5555555555555555555555555555555555555555").expect("valid change id");
        let cp1 = tagged(
            change(1, &idp1, vec![revision(0, &p1, &sha_h, &sha_h)]),
            "s",
            "1",
        );
        let cp2 = tagged(
            change(2, &idp2, vec![revision(0, &p2, &p1, &sha_h)]),
            "s",
            "1",
        );
        let cq = tagged(
            change(3, &idq, vec![revision(0, &sha_q, &p2, &sha_h)]),
            "s",
            "2",
        );
        let cr1 = tagged(
            change(4, &idr1, vec![revision(0, &r1, &sha_h, &sha_h)]),
            "s",
            "2",
        );
        let cr2 = tagged(
            change(5, &idr2, vec![revision(0, &r2, &r1, &sha_h)]),
            "s",
            "2",
        );
        let changes = vec![cp1, cp2, cq, cr1, cr2];
        let history = RepoHistory {
            commits: vec![commit(&sha_h, &[])],
            truncated: false,
        };
        let shas = |g: &ChangeGraph| {
            g.nodes
                .iter()
                .map(|n| n.commit_sha.clone())
                .collect::<Vec<_>>()
        };

        let plain = assemble(&changes, &history, None);
        assert_eq!(
            shas(&plain),
            [&sha_q, &r2, &p2, &r1, &p1, &sha_h].map(Sha::clone)
        );

        let grouped = assemble(&changes, &history, Some("s"));
        assert_eq!(
            shas(&grouped),
            [&sha_q, &r2, &r1, &p2, &p1, &sha_h].map(Sha::clone)
        );
        let groups: Vec<Option<&str>> = grouped.nodes.iter().map(|n| n.group.as_deref()).collect();
        assert_eq!(
            groups,
            [Some("2"), Some("2"), Some("2"), Some("1"), Some("1"), None]
        );

        let other = assemble(&changes, &history, Some("absent"));
        assert_eq!(shas(&other), shas(&plain));
        assert!(other.nodes.iter().all(|n| n.group.is_none()));
    }

    // Two stacks share a tag. Their nodes stay in two runs, each tip
    // above its base, and a change whose parent is not a member's latest
    // revision is a root with no edge.
    #[test]
    fn tag_graph_runs_each_component_together() {
        let sha_m = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let a1 = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let a2 = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let a3 = Sha::new("4444444444444444444444444444444444444444").expect("valid sha");
        let b1 = Sha::new("5555555555555555555555555555555555555555").expect("valid sha");
        let b2 = Sha::new("6666666666666666666666666666666666666666").expect("valid sha");
        let ida1 =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let idb1 =
            ChangeId::new("I2222222222222222222222222222222222222222").expect("valid change id");
        let ida2 =
            ChangeId::new("I3333333333333333333333333333333333333333").expect("valid change id");
        let idb2 =
            ChangeId::new("I4444444444444444444444444444444444444444").expect("valid change id");
        let ida3 =
            ChangeId::new("I5555555555555555555555555555555555555555").expect("valid change id");
        let ca1 = change(1, &ida1, vec![revision(0, &a1, &sha_m, &sha_m)]);
        let cb1 = change(2, &idb1, vec![revision(0, &b1, &sha_m, &sha_m)]);
        let ca2 = change(3, &ida2, vec![revision(0, &a2, &a1, &sha_m)]);
        let cb2 = change(4, &idb2, vec![revision(0, &b2, &b1, &sha_m)]);
        let ca3 = change(5, &ida3, vec![revision(0, &a3, &a2, &sha_m)]);
        let g = assemble_tag(&[ca1, cb1, ca2, cb2, ca3]);
        let shas: Vec<Sha> = g.nodes.iter().map(|n| n.commit_sha.clone()).collect();
        // B2 is the first tip in the input, so B's run leads.
        assert_eq!(shas, [&b2, &b1, &a3, &a2, &a1].map(Sha::clone));
        assert!(!g.history_truncated);
        assert!(g.nodes.iter().all(|n| n.section == GraphSection::Open));
        assert_eq!(g.nodes[1].parents, vec![sha_m]);
    }

    // Only the latest revisions join. A change amended onto a new parent
    // leaves the member that still sits on its old commit as a root, and
    // a component of terminal changes sinks below the active ones.
    #[test]
    fn tag_graph_pins_latest_revisions_and_sinks_terminal_components() {
        let sha_m = Sha::new("1111111111111111111111111111111111111111").expect("valid sha");
        let sha_n = Sha::new("2222222222222222222222222222222222222222").expect("valid sha");
        let sha_o = Sha::new("3333333333333333333333333333333333333333").expect("valid sha");
        let sha_d = Sha::new("4444444444444444444444444444444444444444").expect("valid sha");
        let sha_a = Sha::new("5555555555555555555555555555555555555555").expect("valid sha");
        let ap = Sha::new("6666666666666666666666666666666666666666").expect("valid sha");
        let sha_b = Sha::new("7777777777777777777777777777777777777777").expect("valid sha");
        let idold =
            ChangeId::new("I1111111111111111111111111111111111111111").expect("valid change id");
        let iddead =
            ChangeId::new("I2222222222222222222222222222222222222222").expect("valid change id");
        let ida =
            ChangeId::new("I3333333333333333333333333333333333333333").expect("valid change id");
        let idb =
            ChangeId::new("I4444444444444444444444444444444444444444").expect("valid change id");
        let mut old = change(1, &idold, vec![revision(0, &sha_o, &sha_m, &sha_m)]);
        old.lifecycle = Lifecycle::Merged;
        let mut dead = change(2, &iddead, vec![revision(0, &sha_d, &sha_o, &sha_m)]);
        dead.lifecycle = Lifecycle::Abandoned;
        let ca = change(
            3,
            &ida,
            vec![
                revision(0, &sha_a, &sha_m, &sha_m),
                revision(1, &ap, &sha_n, &sha_n),
            ],
        );
        let cb = change(4, &idb, vec![revision(0, &sha_b, &sha_a, &sha_m)]);
        let g = assemble_tag(&[old, dead, ca, cb]);
        let shas: Vec<Sha> = g.nodes.iter().map(|n| n.commit_sha.clone()).collect();
        assert_eq!(shas, [&ap, &sha_b, &sha_d, &sha_o].map(Sha::clone));
        let node = |sha: &Sha| {
            g.nodes
                .iter()
                .find(|n| &n.commit_sha == sha)
                .unwrap_or_else(|| panic!("no node {sha}"))
        };
        assert_eq!(node(&ap).revision, Some(RevisionNumber::new(1)));
        assert_eq!(node(&sha_d).status, ChangeStatus::Abandoned);
        assert_eq!(node(&sha_o).status, ChangeStatus::Merged);
    }
}
