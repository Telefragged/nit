//! Chain derivation — a chain is **never stored**. Given a repo's per-change
//! folds, [`RepoView`] resolves a commit-sha to `(change, revision)`, walks a
//! tip back to the canonical base through each revision's recorded
//! `parent_sha` (gerrit relation chains), and derives the live tip set and a
//! chain's actionable state. Everything here is a **pure function** of an
//! owned snapshot of the changes (docs/data-model.md "Chain derivation").

use std::collections::{HashMap, HashSet};

use crate::enums::{ChainState, ChangeStatus};
use crate::review::ChangeProj;

/// One member of a derived path, pinned to the revision the walk selected.
#[derive(Debug, Clone)]
pub struct PathMember {
    pub change_id: u64,
    pub revision: u64,
    pub commit_sha: String,
}

/// A tip whose path walks through some change, plus the revision it pins there
/// — drives `ChangeDetail.chains`.
#[derive(Debug, Clone, Copy)]
pub struct ChainHit {
    pub tip_change_id: u64,
    pub revision: u64,
}

/// A read-time view over one repo's changes: owned snapshots plus the
/// commit-sha → `(change_id, revision number)` index built from them. All
/// chain derivation is a pure function of this view, so it holds no locks and
/// touches no git.
pub struct RepoView {
    changes: HashMap<u64, ChangeProj>,
    index: HashMap<String, (u64, u64)>,
}

impl RepoView {
    /// Build the view from owned change snapshots (each cloned out from under
    /// its lock by the caller, so the view holds nothing live).
    #[must_use]
    pub fn new(changes: Vec<ChangeProj>) -> RepoView {
        let mut index = HashMap::new();
        let mut map = HashMap::new();
        for c in changes {
            for rev in &c.revisions {
                index.insert(rev.commit_sha.clone(), (c.id, rev.number));
            }
            map.insert(c.id, c);
        }
        RepoView {
            changes: map,
            index,
        }
    }

    #[must_use]
    pub fn change(&self, id: u64) -> Option<&ChangeProj> {
        self.changes.get(&id)
    }

    /// Every change id in the view.
    #[must_use]
    pub fn change_ids(&self) -> Vec<u64> {
        self.changes.keys().copied().collect()
    }

    /// Tips including terminal changes' leaves — the `status=all` view, which
    /// still surfaces recently merged/abandoned chains.
    #[must_use]
    pub fn all_tips(&self) -> Vec<String> {
        let parents: HashSet<&str> = self
            .changes
            .values()
            .flat_map(|c| c.revisions.iter().map(|r| r.parent_sha.as_str()))
            .collect();
        let mut tips: Vec<String> = self
            .changes
            .values()
            .filter_map(ChangeProj::latest_revision)
            .map(|r| r.commit_sha.clone())
            .filter(|sha| !parents.contains(sha.as_str()))
            .collect();
        tips.sort();
        tips
    }

    /// The set of live tip commit-shas: each non-terminal change's
    /// latest-revision sha that no revision records as a `parent_sha` (a leaf
    /// in the parent DAG). A superseded patchset is never a tip (only the
    /// latest revision is a candidate); a terminal change is not a tip but can
    /// still be an interior member of a live tip's path. Sorted for a stable
    /// order.
    #[must_use]
    pub fn tips(&self) -> Vec<String> {
        let parents: HashSet<&str> = self
            .changes
            .values()
            .flat_map(|c| c.revisions.iter().map(|r| r.parent_sha.as_str()))
            .collect();
        let mut tips: Vec<String> = self
            .changes
            .values()
            .filter(|c| !c.is_terminal())
            .filter_map(ChangeProj::latest_revision)
            .map(|r| r.commit_sha.clone())
            .filter(|sha| !parents.contains(sha.as_str()))
            .collect();
        tips.sort();
        tips
    }

    /// Walk a tip commit-sha back to the canonical base through each revision's
    /// recorded `parent`, returning the path oldest-first. **Total**: an
    /// unresolved parent (below the merge-base, or a torn push) truncates the
    /// path, never errors.
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
            let Some(rev) = self.change(change_id).and_then(|c| c.revision(number)) else {
                break;
            };
            if rev.parent_sha == rev.base_sha {
                break; // the fork point on the canonical branch
            }
            sha.clone_from(&rev.parent_sha);
        }
        path.reverse();
        path
    }

    /// The tips whose path walks through `change_id`, each with the revision
    /// that path pins on it.
    #[must_use]
    pub fn chains_through(&self, change_id: u64) -> Vec<ChainHit> {
        let mut hits = Vec::new();
        for tip in self.tips() {
            let path = self.path_from_tip(&tip);
            let Some(member) = path.iter().find(|m| m.change_id == change_id) else {
                continue;
            };
            let tip_change_id = path.last().map_or(change_id, |m| m.change_id);
            hits.push(ChainHit {
                tip_change_id,
                revision: member.revision,
            });
        }
        hits
    }
}

/// Whether a path is partial: its **tip** change's latest revision is partial
/// (the tip is the work frontier — the most recent push's intent governs).
#[must_use]
pub fn is_partial(view: &RepoView, path: &[PathMember]) -> bool {
    path.last()
        .and_then(|m| view.change(m.change_id))
        .is_some_and(ChangeProj::is_partial)
}

/// Derived chain state over a path's members, each at its pinned revision
/// (docs/api.md state table). A pure function of the members' displayed status
/// plus the tip's partial flag.
#[must_use]
pub fn derive_state(view: &RepoView, path: &[PathMember]) -> ChainState {
    if path.is_empty() {
        return ChainState::AgentsTurn; // empty tip
    }
    let statuses: Vec<ChangeStatus> = path
        .iter()
        .map(|m| {
            view.change(m.change_id)
                .map_or(ChangeStatus::Pending, |c| c.status_at(m.revision))
        })
        .collect();
    if statuses.iter().all(|s| *s == ChangeStatus::Merged) {
        ChainState::Merged
    } else if statuses.contains(&ChangeStatus::Abandoned) {
        ChainState::HasAbandoned
    } else if statuses
        .iter()
        .any(|s| matches!(s, ChangeStatus::ChangesRequested | ChangeStatus::Commented))
    {
        ChainState::AgentsTurn
    } else if statuses.contains(&ChangeStatus::Pending) {
        ChainState::WaitingForReview
    } else if is_partial(view, path) {
        ChainState::AgentsTurn // all approved but still pushing
    } else {
        ChainState::Approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ChangeRow;
    use crate::enums::Verdict;
    use crate::review::{ChangeProj, ReviewProj, RevisionProj};

    fn row(id: u64, key: &str) -> ChangeRow {
        ChangeRow {
            id,
            repo_id: 1,
            change_key: key.to_string(),
            created_at: "t0".to_string(),
        }
    }

    fn rev(number: u64, sha: &str, parent: &str, base: &str) -> RevisionProj {
        RevisionProj {
            number,
            commit_sha: sha.to_string(),
            parent_sha: parent.to_string(),
            base_sha: base.to_string(),
            message: format!("subject {sha}"),
            partial: false,
            resets_status: true,
            created_at: "t0".to_string(),
        }
    }

    fn change(id: u64, key: &str, revs: Vec<RevisionProj>) -> ChangeProj {
        let mut c = ChangeProj::empty(&row(id, key));
        c.revisions = revs;
        c
    }

    /// push1 `m → A → B → C` and push2 `m → D → B' → E`: B is one change at two
    /// patchsets, surfaced as two tips/chains.
    #[test]
    fn b_in_two_chains() {
        let ca = change(10, "Ia", vec![rev(0, "A", "m", "m")]);
        // B: rev0 parent A (push1), rev1 parent D (push2).
        let cb = change(
            11,
            "Ib",
            vec![rev(0, "B", "A", "m"), rev(1, "Bp", "D", "m")],
        );
        let cc = change(12, "Ic", vec![rev(0, "C", "B", "m")]);
        let cd = change(13, "Id", vec![rev(0, "D", "m", "m")]);
        let ce = change(14, "Ie", vec![rev(0, "E", "Bp", "m")]);
        let view = RepoView::new(vec![ca, cb, cc, cd, ce]);

        // Tips are C and E (leaves over latest revisions).
        assert_eq!(view.tips(), vec!["C".to_string(), "E".to_string()]);

        // The C-chain walks B at rev0; the E-chain walks B at rev1.
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

        // B (change 11) is reached by both tips, at two patchsets.
        let hits = view.chains_through(11);
        let mut pairs: Vec<(u64, u64)> =
            hits.iter().map(|h| (h.tip_change_id, h.revision)).collect();
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(12, 0), (14, 1)]);
    }

    #[test]
    fn prefix_branch_is_subsumed() {
        // m → A → B, then extended to m → A → B → C: only C is a tip.
        let a = change(1, "Ia", vec![rev(0, "A", "m", "m")]);
        let b = change(2, "Ib", vec![rev(0, "B", "A", "m")]);
        let c = change(3, "Ic", vec![rev(0, "C", "B", "m")]);
        let view = RepoView::new(vec![a, b, c]);
        assert_eq!(view.tips(), vec!["C".to_string()]);
    }

    #[test]
    fn state_is_derived_from_members() {
        let mut a = change(1, "Ia", vec![rev(0, "A", "m", "m")]);
        a.reviews.push(ReviewProj {
            id: 100,
            revision: 0,
            verdict: Verdict::Approve,
            message: String::new(),
            created_at: "t1".to_string(),
        });
        let b = change(2, "Ib", vec![rev(0, "B", "A", "m")]); // pending
        let view = RepoView::new(vec![a, b]);
        let path = view.path_from_tip("B");
        // A approved, B pending → waiting_for_review.
        assert_eq!(derive_state(&view, &path), ChainState::WaitingForReview);
    }
}
