//! View assembly: db rows (+ git trees for comment porting) → the wire
//! shapes of docs/api.md. All functions are blocking (rusqlite/git2) —
//! handlers call them inside `spawn_blocking`.

use std::collections::HashMap;

use anyhow::Result;
use git2::{Oid, Repository};
use rusqlite::Connection;

use crate::db::{self, ChainStatus, ChangeStatus};
use crate::gitscan::fixup::subject_of;

use super::diff;
use super::types;

pub fn short_sha(sha: &str) -> String {
    sha.chars().take(12).collect()
}

// ---------------------------------------------------------------------------
// Chain state (derived, never stored) — docs/data-model.md status machine

/// One live change's inputs to the state derivation.
struct LiveChange {
    status: ChangeStatus,
    needs_rebase: bool,
}

fn derive_state(status: ChainStatus, live: &[LiveChange]) -> &'static str {
    match status {
        ChainStatus::Merged => "merged",
        ChainStatus::Abandoned => "abandoned",
        ChainStatus::Active => {
            if live.is_empty() {
                return "agents_turn"; // empty chain
            }
            if live.iter().any(|c| {
                c.needs_rebase
                    || matches!(
                        c.status,
                        ChangeStatus::ChangesRequested | ChangeStatus::Commented
                    )
            }) {
                "agents_turn"
            } else if live
                .iter()
                .any(|c| !matches!(c.status, ChangeStatus::Approved))
            {
                "waiting_for_review"
            } else {
                "ready_to_merge"
            }
        }
    }
}

fn actionable(state: &str) -> bool {
    state != "waiting_for_review"
}

// ---------------------------------------------------------------------------
// Chain + ChangeSummary

pub fn build_chain(
    conn: &Connection,
    public_base: &str,
    warnings: Vec<String>,
    chain: &db::Chain,
) -> Result<types::Chain> {
    let repo_path = db::chain_repo_path(conn, chain.id)?.unwrap_or_default();
    let mut summaries = Vec::new();
    let mut live = Vec::new();
    for change in db::changes_for_chain(conn, chain.id)? {
        let Some(summary) = change_summary(conn, &change)? else {
            continue; // defensive: a change row always has revisions
        };
        if change.position.is_some() {
            live.push(LiveChange {
                status: change.status,
                needs_rebase: summary.needs_rebase,
            });
        }
        summaries.push(summary);
    }
    Ok(types::Chain {
        id: chain.id,
        repo_path,
        branch: chain.branch.clone(),
        base: chain.base.clone(),
        status: chain.status.as_str().to_string(),
        state: derive_state(chain.status, &live).to_string(),
        last_scan_error: chain.last_scan_error.clone(),
        scan_warnings: warnings,
        web_url: format!("{public_base}/chains/{}", chain.id),
        created_at: chain.created_at.clone(),
        updated_at: chain.updated_at.clone(),
        changes: summaries,
    })
}

fn change_summary(conn: &Connection, change: &db::Change) -> Result<Option<types::ChangeSummary>> {
    let Some(latest) = db::latest_revision(conn, change.id)? else {
        return Ok(None);
    };
    let (published_comments, drafts, unresolved) = db::comment_counts(conn, change.id)?;
    Ok(Some(types::ChangeSummary {
        id: change.id,
        position: change.position,
        change_key: change.change_key.clone(),
        subject: subject_of(&latest.message),
        status: change.status.as_str().to_string(),
        revision: latest.number,
        last_reviewed_revision: db::last_reviewed_revision(conn, change.id)?,
        commit_sha: latest.commit_sha.clone(),
        short_sha: short_sha(&latest.commit_sha),
        needs_rebase: latest.effective_tree.is_none(),
        counts: types::ChangeCounts {
            revisions: latest.number,
            published_comments,
            drafts,
            unresolved,
        },
    }))
}

// ---------------------------------------------------------------------------
// Comment rendering across revisions (docs/api.md)

/// Ports comment anchors of one change onto a target revision. `repo:
/// None` (repository unopenable) renders cross-revision anchors as
/// outdated rather than failing the whole response.
pub struct CommentRenderer<'a> {
    conn: &'a Connection,
    repo: Option<&'a Repository>,
    change_id: i64,
    target: i64,
    /// (revision, `is_new_side`) → tree oid, None = unresolvable.
    trees: HashMap<(i64, bool), Option<Oid>>,
}

impl<'a> CommentRenderer<'a> {
    pub fn new(
        conn: &'a Connection,
        repo: Option<&'a Repository>,
        change_id: i64,
        target: i64,
    ) -> Self {
        CommentRenderer {
            conn,
            repo,
            change_id,
            target,
            trees: HashMap::new(),
        }
    }

    pub fn render(&mut self, comment: &db::Comment) -> types::Comment {
        let (rendered_line, outdated) = self.port(comment);
        comment_json(comment, rendered_line, outdated)
    }

    /// `(rendered_line, outdated)` for the target revision.
    fn port(&mut self, comment: &db::Comment) -> (Option<i64>, bool) {
        let (Some(file), Some(line)) = (comment.file.as_deref(), comment.line) else {
            return (None, false); // change-/file-level: never outdated
        };
        if comment.revision_number == self.target {
            return (Some(line), false);
        }
        let new_side = comment.side != "old";
        let Some(repo) = self.repo else {
            return (None, true);
        };
        let (Some(from), Some(to)) = (
            self.tree_oid(comment.revision_number, new_side),
            self.tree_oid(self.target, new_side),
        ) else {
            return (None, true); // fold conflict / pruned objects
        };
        let ported = repo
            .find_tree(from)
            .and_then(|f| repo.find_tree(to).map(|t| (f, t)))
            .ok()
            .and_then(|(f, t)| diff::port_line(repo, &f, &t, file, line).ok())
            .flatten();
        match ported {
            Some(l) => (Some(l), false),
            None => (None, true),
        }
    }

    /// The tree a side of a revision's diff shows: effective tree (new) or
    /// the parent commit's tree (old — deleted lines live there).
    fn tree_oid(&mut self, revision: i64, new_side: bool) -> Option<Oid> {
        if let Some(cached) = self.trees.get(&(revision, new_side)) {
            return *cached;
        }
        let oid = self.lookup_tree(revision, new_side);
        self.trees.insert((revision, new_side), oid);
        oid
    }

    fn lookup_tree(&self, revision: i64, new_side: bool) -> Option<Oid> {
        let rev = db::get_revision(self.conn, self.change_id, revision)
            .ok()
            .flatten()?;
        let repo = self.repo?;
        if new_side {
            let oid = Oid::from_str(rev.effective_tree.as_deref()?).ok()?;
            repo.find_tree(oid).ok().map(|t| t.id())
        } else {
            let parent = repo
                .find_commit(Oid::from_str(&rev.parent_sha).ok()?)
                .ok()?;
            Some(parent.tree_id())
        }
    }
}

fn comment_json(c: &db::Comment, rendered_line: Option<i64>, outdated: bool) -> types::Comment {
    types::Comment {
        id: c.id,
        change_id: c.change_id,
        revision: c.revision_number,
        parent_id: c.parent_id,
        author: c.author.clone(),
        file: c.file.clone(),
        line: c.line,
        side: c.side.clone(),
        line_text: c.line_text.clone(),
        rendered_line,
        outdated,
        body: c.body.clone(),
        state: c.state.clone(),
        resolved: c.resolved,
        review_id: c.review_id,
        created_at: c.created_at.clone(),
        updated_at: c.updated_at.clone(),
    }
}

/// A comment rendered at its own revision (draft CRUD / reply / publish
/// responses — porting happens when the change is *viewed*).
pub fn comment_at_own_revision(c: &db::Comment) -> types::Comment {
    comment_json(c, c.line, false)
}

// ---------------------------------------------------------------------------
// Change detail

pub fn build_change_detail(
    conn: &Connection,
    repo: Option<&Repository>,
    change: &db::Change,
    requested_revision: i64,
) -> Result<types::ChangeDetail> {
    let revisions: Vec<types::Revision> = db::revisions_for_change(conn, change.id)?
        .iter()
        .map(revision_json)
        .collect();
    let mut renderer = CommentRenderer::new(conn, repo, change.id, requested_revision);
    let comments = db::comments_for_change(conn, change.id)?
        .iter()
        .map(|c| renderer.render(c))
        .collect();
    let reviews = db::reviews_for_change(conn, change.id)?
        .iter()
        .map(review_json)
        .collect();
    let subject = revisions
        .last()
        .map(|r| subject_of(&r.message))
        .unwrap_or_default();
    Ok(types::ChangeDetail {
        id: change.id,
        chain_id: change.chain_id,
        change_key: change.change_key.clone(),
        position: change.position,
        status: change.status.as_str().to_string(),
        subject,
        last_reviewed_revision: db::last_reviewed_revision(conn, change.id)?,
        revisions,
        comments,
        reviews,
    })
}

pub fn revision_json(rev: &db::Revision) -> types::Revision {
    types::Revision {
        number: rev.number,
        commit_sha: rev.commit_sha.clone(),
        short_sha: short_sha(&rev.commit_sha),
        parent_sha: rev.parent_sha.clone(),
        message: rev.message.clone(),
        fixups: rev
            .fixups
            .iter()
            .map(|f| types::RevisionFixup {
                sha: f.sha.clone(),
                short_sha: short_sha(&f.sha),
                message: f.message.clone(),
            })
            .collect(),
        needs_rebase: rev.effective_tree.is_none(),
        created_at: rev.created_at.clone(),
    }
}

pub fn review_json(review: &db::Review) -> types::Review {
    types::Review {
        id: review.id,
        revision: review.revision_number,
        verdict: review.verdict.clone(),
        message: review.message.clone(),
        created_at: review.created_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Feedback (agent side)

pub fn build_feedback(
    conn: &Connection,
    repo: Option<&Repository>,
    public_base: &str,
    warnings: Vec<String>,
    chain: &db::Chain,
) -> Result<types::Feedback> {
    let mut live = Vec::new();
    let mut changes = Vec::new();
    for change in db::changes_for_chain(conn, chain.id)? {
        if change.position.is_none() {
            continue; // live changes only
        }
        let Some(latest) = db::latest_revision(conn, change.id)? else {
            continue;
        };
        let needs_rebase = latest.effective_tree.is_none();
        live.push(LiveChange {
            status: change.status,
            needs_rebase,
        });

        let latest_review = db::latest_review_for_change(conn, change.id)?;
        let comments = feedback_comments(conn, repo, &change, latest.number, &latest_review)?;
        let (_, _, unresolved) = db::comment_counts(conn, change.id)?;
        changes.push(types::FeedbackChange {
            change_id: change.id,
            change_key: change.change_key.clone(),
            subject: subject_of(&latest.message),
            commit_sha: latest.commit_sha.clone(),
            revision: latest.number,
            status: change.status.as_str().to_string(),
            needs_rebase,
            unresolved,
            review: latest_review.as_ref().map(|r| types::FeedbackReview {
                verdict: r.verdict.clone(),
                message: r.message.clone(),
                revision: r.revision_number,
            }),
            comments,
        });
    }
    let state = derive_state(chain.status, &live);
    Ok(types::Feedback {
        state: state.to_string(),
        actionable: actionable(state),
        chain: types::FeedbackChain {
            id: chain.id,
            branch: chain.branch.clone(),
            base: chain.base.clone(),
            web_url: format!("{public_base}/chains/{}", chain.id),
            last_scan_error: chain.last_scan_error.clone(),
            scan_warnings: warnings,
        },
        changes,
    })
}

/// Feedback comment scope: the latest review's comments, plus
/// still-unresolved published threads from earlier reviews — each thread
/// whole (root + replies), rendered at the latest revision.
fn feedback_comments(
    conn: &Connection,
    repo: Option<&Repository>,
    change: &db::Change,
    latest_revision: i64,
    latest_review: &Option<db::Review>,
) -> Result<Vec<types::Comment>> {
    let all = db::comments_for_change(conn, change.id)?;
    let in_scope_root = |root: &db::Comment| -> bool {
        if root.state != "published" {
            return false;
        }
        if latest_review
            .as_ref()
            .is_some_and(|r| root.review_id == Some(r.id))
        {
            return true;
        }
        !root.resolved
    };
    let roots: std::collections::HashSet<i64> = all
        .iter()
        .filter(|c| c.parent_id.is_none() && in_scope_root(c))
        .map(|c| c.id)
        .collect();
    let mut renderer = CommentRenderer::new(conn, repo, change.id, latest_revision);
    Ok(all
        .iter()
        .filter(|c| {
            c.state == "published"
                && (roots.contains(&c.id) || c.parent_id.is_some_and(|p| roots.contains(&p)))
        })
        .map(|c| renderer.render(c))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_table() {
        let live = |spec: &[(&str, bool)]| -> Vec<LiveChange> {
            spec.iter()
                .map(|(s, nr)| LiveChange {
                    status: ChangeStatus::parse(s).unwrap(),
                    needs_rebase: *nr,
                })
                .collect()
        };
        assert_eq!(derive_state(ChainStatus::Merged, &[]), "merged");
        assert_eq!(derive_state(ChainStatus::Abandoned, &[]), "abandoned");
        assert_eq!(derive_state(ChainStatus::Active, &[]), "agents_turn");
        assert_eq!(
            derive_state(ChainStatus::Active, &live(&[("pending", false)])),
            "waiting_for_review"
        );
        assert_eq!(
            derive_state(
                ChainStatus::Active,
                &live(&[("approved", false), ("changes_requested", false)])
            ),
            "agents_turn"
        );
        assert_eq!(
            derive_state(ChainStatus::Active, &live(&[("commented", false)])),
            "agents_turn"
        );
        assert_eq!(
            derive_state(ChainStatus::Active, &live(&[("pending", true)])),
            "agents_turn" // needs_rebase wins over pending
        );
        assert_eq!(
            derive_state(
                ChainStatus::Active,
                &live(&[("approved", false), ("approved", false)])
            ),
            "ready_to_merge"
        );
        assert_eq!(
            derive_state(
                ChainStatus::Active,
                &live(&[("approved", false), ("pending", false)])
            ),
            "waiting_for_review"
        );
        assert!(actionable("agents_turn"));
        assert!(actionable("merged"));
        assert!(!actionable("waiting_for_review"));
    }

    #[test]
    fn short_sha_truncates() {
        assert_eq!(short_sha(&"a".repeat(40)), "a".repeat(12));
        assert_eq!(short_sha("abc"), "abc");
    }
}
