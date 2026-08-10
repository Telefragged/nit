//! Chain derivation — a chain is **never stored**.
//!
//! Given a repo's per-change folds, [`RepoView`] resolves a commit-sha to
//! `(change, revision)`, walks a tip back to the canonical ref through
//! each revision's recorded `parent_sha` (gerrit relation chains), and
//! derives the live tip set and a chain's actionable state. Everything
//! here is a **pure function** of an owned projection of the changes.

use std::collections::{HashMap, HashSet};

use crate::domain::{ChainState, ChangeStatus};

use crate::fold::ChangeProjection;

/// One member of a derived path, pinned to the revision the walk selected.
#[derive(Debug, Clone)]
pub struct PathMember {
    pub change_id: u64,
    pub revision: u64,
    pub commit_sha: String,
}

/// One node of the graph's open region.
///
/// An active change pinned at the revision its tip walked, plus the
/// commit it parents onto.
#[derive(Debug, Clone)]
pub struct OpenNode {
    pub change_id: u64,
    pub revision: u64,
    pub commit_sha: String,
    pub parent_sha: String,
}

/// A read-time view over one repo's changes.
///
/// Owned projections plus the commit-sha → `(change_id, revision number)`
/// index built from them. All chain derivation is a pure function of
/// this view, so it holds no locks and touches no git.
pub struct RepoView {
    changes: HashMap<u64, ChangeProjection>,
    index: HashMap<String, (u64, u64)>,
}

impl RepoView {
    /// Builds the view from owned change projections.
    ///
    /// Each is cloned out from under its lock by the caller, so the view
    /// holds nothing live.
    #[must_use]
    pub fn new(changes: Vec<ChangeProjection>) -> RepoView {
        let mut index = HashMap::new();
        let mut map = HashMap::new();
        for c in changes {
            for revision in &c.revisions {
                index.insert(revision.commit_sha.clone(), (c.id, revision.number));
            }
            map.insert(c.id, c);
        }
        RepoView {
            changes: map,
            index,
        }
    }

    #[must_use]
    pub fn change(&self, id: u64) -> Option<&ChangeProjection> {
        self.changes.get(&id)
    }

    #[must_use]
    pub fn change_ids(&self) -> Vec<u64> {
        self.changes.keys().copied().collect()
    }

    /// Tip commit-shas over the changes `keep` admits, sorted.
    ///
    /// A tip is a change's latest-revision sha that no revision records
    /// as a `parent_sha`. A superseded revision is never a tip — only
    /// the latest revision is a candidate.
    fn tips_where(&self, keep: impl Fn(&ChangeProjection) -> bool) -> Vec<String> {
        let parents: HashSet<&str> = self
            .changes
            .values()
            .flat_map(|c| c.revisions.iter().map(|r| r.parent_sha.as_str()))
            .collect();
        let mut tips: Vec<String> = self
            .changes
            .values()
            .filter(|&c| keep(c))
            .filter_map(ChangeProjection::latest_revision)
            .map(|r| r.commit_sha.clone())
            .filter(|sha| !parents.contains(sha.as_str()))
            .collect();
        tips.sort();
        tips
    }

    /// Every tip — the `status=all` view.
    ///
    /// It still surfaces recently merged/abandoned chains.
    #[must_use]
    pub fn all_tips(&self) -> Vec<String> {
        self.tips_where(|_| true)
    }

    /// The **active frontier**: tips of non-terminal changes.
    ///
    /// The dashboard's `status=active`. A merged change is on the canonical ref and an
    /// abandoned change is dead, so neither is an active tip — but an
    /// abandoned change is still an enumerable member
    /// ([`enumerable_tips`](Self::enumerable_tips)).
    #[must_use]
    pub fn tips(&self) -> Vec<String> {
        self.tips_where(|c| !c.is_terminal())
    }

    /// Tips for **chain enumeration**: drops only merged changes.
    ///
    /// An abandoned tip therefore still resolves to its own chain
    /// (abandonment is membership-inert). The dashboard hides these via
    /// [`tips`](Self::tips); resolving the chain a change sits on
    /// enumerates them.
    #[must_use]
    pub fn enumerable_tips(&self) -> Vec<String> {
        self.tips_where(|c| !c.is_merged())
    }

    /// Walks a tip commit-sha back to the canonical ref.
    ///
    /// Follows each revision's recorded `parent`, returning the path
    /// oldest-first. The walk stops at the branch: the recorded fork
    /// (`parent_sha == fork_sha`), or the first parent that has since
    /// merged — so a partially-merged stack derives to its open members
    /// alone. **Total**: an unresolved parent (below the merge-base, or a
    /// torn push) truncates the path, never errors.
    #[must_use]
    pub fn path_from_tip(&self, tip_sha: &str) -> Vec<PathMember> {
        let mut path = Vec::new();
        let mut sha = tip_sha.to_string();
        let mut seen = HashSet::new();
        while let Some(&(change_id, number)) = self.index.get(&sha) {
            if !seen.insert(sha.clone()) {
                break; // cycle guard against bad data
            }
            path.push(PathMember {
                change_id,
                revision: number,
                commit_sha: sha.clone(),
            });
            let Some(revision) = self.change(change_id).and_then(|c| c.revision(number)) else {
                break;
            };
            if revision.parent_sha == revision.fork_sha || self.is_merged(&revision.parent_sha) {
                break;
            }
            sha.clone_from(&revision.parent_sha);
        }
        path.reverse();
        path
    }

    /// Whether `sha` is a change that has merged onto the canonical ref.
    fn is_merged(&self, sha: &str) -> bool {
        self.index
            .get(sha)
            .and_then(|&(id, _)| self.change(id))
            .is_some_and(ChangeProjection::is_merged)
    }

    /// A change by its `Change-Id` key.
    ///
    /// The graph enriches a merged history commit from its commit-message
    /// trailer.
    #[must_use]
    pub fn change_by_key(&self, key: &str) -> Option<&ChangeProjection> {
        self.changes.values().find(|c| c.change_key.as_str() == key)
    }

    /// The graph's **open region**: active tips walked back to their forks.
    ///
    /// Unioned and **deduplicated by commit-sha** — a change shared by two
    /// tips appears once, while the rare B-in-two-chains case (one change
    /// live at two revisions) stays two nodes (two shas). In tip-walk
    /// order, which seeds the graph's row-order tie-break.
    #[must_use]
    pub fn open_nodes(&self) -> Vec<OpenNode> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for tip in self.tips() {
            for m in self.path_from_tip(&tip) {
                if !seen.insert(m.commit_sha.clone()) {
                    continue;
                }
                let parent_sha = self
                    .change(m.change_id)
                    .and_then(|c| c.revision(m.revision))
                    .map_or_else(String::new, |r| r.parent_sha.clone());
                out.push(OpenNode {
                    change_id: m.change_id,
                    revision: m.revision,
                    commit_sha: m.commit_sha,
                    parent_sha,
                });
            }
        }
        out
    }
}

/// Derived chain state over a path's members.
///
/// Each member is taken at its pinned revision. A pure function of the
/// members' displayed status.
#[must_use]
pub fn derive_state(view: &RepoView, path: &[PathMember]) -> ChainState {
    if path.is_empty() {
        return ChainState::AuthorsTurn; // tip not in the index — no chain members, author's turn by default
    }
    // Abandonment is derivation-inert: an abandoned member is excluded from the
    // rollup entirely (no chain-level abandoned state). It shows as `abandoned`
    // on its own path entry; the author decides what to do with it.
    let statuses: Vec<ChangeStatus> = path
        .iter()
        .map(|m| {
            view.change(m.change_id)
                .map_or(ChangeStatus::Pending, |c| c.status_at(m.revision))
        })
        .filter(|s| *s != ChangeStatus::Abandoned)
        .collect();
    if statuses.is_empty() {
        return ChainState::AuthorsTurn;
    }
    if statuses.iter().all(|s| *s == ChangeStatus::Merged) {
        ChainState::Merged
    } else if statuses
        .iter()
        .any(|s| matches!(s, ChangeStatus::ChangesRequested | ChangeStatus::Commented))
    {
        ChainState::AuthorsTurn
    } else if statuses.contains(&ChangeStatus::Pending) {
        ChainState::WaitingForReview
    } else {
        ChainState::Approved
    }
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
pub fn graph_row_order(nodes: &[(String, Vec<String>)]) -> Vec<String> {
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

    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (sha, _))| (sha.as_str(), i))
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, (_, parents)) in nodes.iter().enumerate() {
        for p in parents {
            if let Some(&pi) = index.get(p.as_str()) {
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
    use crate::domain::Verdict;

    use crate::fold::{ChangeProjection, Lifecycle, ReviewProjection, RevisionProjection};

    fn revision(number: u64, sha: &str, parent: &str, base: &str) -> RevisionProjection {
        RevisionProjection {
            number,
            commit_sha: sha.to_string(),
            parent_sha: parent.to_string(),
            fork_sha: base.to_string(),
            message: format!("subject {sha}"),
            resets_status: true,
            created_at: "t0".to_string(),
        }
    }

    fn change(id: u64, key: &str, revs: Vec<RevisionProjection>) -> ChangeProjection {
        let mut c = ChangeProjection::new(id, 1, key.into());
        c.revisions = revs;
        c
    }

    /// push1 `m → A → B → C` and push2 `m → D → B' → E`: B is one change at two
    /// revisions, surfaced as two tips/chains.
    #[test]
    fn b_in_two_chains() {
        let ca = change(10, "Ia", vec![revision(0, "A", "m", "m")]);
        let cb = change(
            11,
            "Ib",
            vec![revision(0, "B", "A", "m"), revision(1, "Bp", "D", "m")],
        );
        let cc = change(12, "Ic", vec![revision(0, "C", "B", "m")]);
        let cd = change(13, "Id", vec![revision(0, "D", "m", "m")]);
        let ce = change(14, "Ie", vec![revision(0, "E", "Bp", "m")]);
        let view = RepoView::new(vec![ca, cb, cc, cd, ce]);

        assert_eq!(view.tips(), vec!["C".to_string(), "E".to_string()]);

        let c_path = view.path_from_tip("C");
        assert_eq!(
            c_path
                .iter()
                .map(|m| (m.change_id, m.revision))
                .collect::<Vec<_>>(),
            vec![(10, 0), (11, 0), (12, 0)]
        );
        let e_path = view.path_from_tip("E");
        assert_eq!(
            e_path
                .iter()
                .map(|m| (m.change_id, m.revision))
                .collect::<Vec<_>>(),
            vec![(13, 0), (11, 1), (14, 0)]
        );
    }

    #[test]
    fn walk_stops_at_a_merged_ancestor() {
        // A → B forked from "m"; A has since merged. The walk stops at
        // the canonical ref, so B's path is the open member alone — the
        // merged ancestor sits below the branch now, not in the chain.
        let mut a = change(1, "Ia", vec![revision(0, "A", "m", "m")]);
        a.lifecycle = Lifecycle::Merged;
        let b = change(2, "Ib", vec![revision(0, "B", "A", "m")]);
        let view = RepoView::new(vec![a, b]);

        let path: Vec<u64> = view
            .path_from_tip("B")
            .iter()
            .map(|m| m.change_id)
            .collect();
        assert_eq!(
            path,
            vec![2],
            "the merged ancestor is below the canonical ref"
        );
        // The graph's open region inherits the stop — no merged node leaks in.
        let open: Vec<u64> = view.open_nodes().iter().map(|n| n.change_id).collect();
        assert_eq!(open, vec![2]);
    }

    #[test]
    fn prefix_branch_is_subsumed() {
        let a = change(1, "Ia", vec![revision(0, "A", "m", "m")]);
        let b = change(2, "Ib", vec![revision(0, "B", "A", "m")]);
        let c = change(3, "Ic", vec![revision(0, "C", "B", "m")]);
        let view = RepoView::new(vec![a, b, c]);
        assert_eq!(view.tips(), vec!["C".to_string()]);
    }

    #[test]
    fn open_nodes_dedupes_shared_change_keeps_two_revisions() {
        // The b_in_two_chains topology: B is one change at two revisions under
        // two live tips. Open nodes dedupe by sha, so B@rev0 (sha "B") and
        // B@rev1 (sha "Bp") are two nodes — different commits, different parents.
        let ca = change(10, "Ia", vec![revision(0, "A", "m", "m")]);
        let cb = change(
            11,
            "Ib",
            vec![revision(0, "B", "A", "m"), revision(1, "Bp", "D", "m")],
        );
        let cc = change(12, "Ic", vec![revision(0, "C", "B", "m")]);
        let cd = change(13, "Id", vec![revision(0, "D", "m", "m")]);
        let ce = change(14, "Ie", vec![revision(0, "E", "Bp", "m")]);
        let view = RepoView::new(vec![ca, cb, cc, cd, ce]);

        let mut shas: Vec<String> = view
            .open_nodes()
            .iter()
            .map(|n| n.commit_sha.clone())
            .collect();
        shas.sort();
        assert_eq!(shas, vec!["A", "B", "Bp", "C", "D", "E"]);
        let b_nodes: Vec<u64> = view
            .open_nodes()
            .iter()
            .filter(|n| n.change_id == 11)
            .map(|n| n.revision)
            .collect();
        assert_eq!(b_nodes.len(), 2);
        assert!(b_nodes.contains(&0) && b_nodes.contains(&1));
    }

    #[test]
    fn graph_row_order_is_topological_children_before_parents() {
        // The change-graph mock topology: two open tips (A1, A2) fanning from
        // A3 → A4 → HEAD, then merged history H → G1 → G2(merge of G3,G4) → G5.
        let pairs = vec![
            ("A1".to_string(), vec!["A3".to_string()]),
            ("A2".to_string(), vec!["A3".to_string()]),
            ("A3".to_string(), vec!["A4".to_string()]),
            ("A4".to_string(), vec!["H".to_string()]),
            ("H".to_string(), vec!["G1".to_string()]),
            ("G1".to_string(), vec!["G2".to_string()]),
            ("G2".to_string(), vec!["G3".to_string(), "G4".to_string()]),
            ("G3".to_string(), vec!["G5".to_string()]),
            ("G4".to_string(), vec!["G5".to_string()]),
            ("G5".to_string(), vec![]),
        ];
        assert_eq!(
            graph_row_order(&pairs),
            vec!["A1", "A2", "A3", "A4", "H", "G1", "G2", "G3", "G4", "G5"]
        );
        // None < Some keeps the comparison honest if a sha is ever missing.
        let order = graph_row_order(&pairs);
        let pos = |s: &str| order.iter().position(|x| x == s);
        for (child, parents) in &pairs {
            for p in parents {
                assert!(pos(child) < pos(p), "{child} should precede parent {p}");
            }
        }
    }

    #[test]
    fn state_is_derived_from_members() {
        let mut a = change(1, "Ia", vec![revision(0, "A", "m", "m")]);
        a.reviews.push(ReviewProjection {
            id: 100,
            revision: 0,
            verdict: Verdict::Approve,
            message: String::new(),
            created_at: "t1".to_string(),
        });
        let b = change(2, "Ib", vec![revision(0, "B", "A", "m")]);
        let view = RepoView::new(vec![a, b]);
        let path = view.path_from_tip("B");
        assert_eq!(derive_state(&view, &path), ChainState::WaitingForReview);
    }
}
