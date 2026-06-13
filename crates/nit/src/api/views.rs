//! View assembly: db rows → the wire shapes of docs/api.md. All functions
//! are blocking (rusqlite) — handlers call them inside `spawn_blocking`.

use anyhow::Result;
use rusqlite::Connection;

use crate::db::{self, ChainStatus, ChangeStatus};
use crate::gitscan::identity::subject_of;

use super::types;

#[must_use]
pub fn short_sha(sha: &str) -> String {
    sha.chars().take(12).collect()
}

// ---------------------------------------------------------------------------
// Chain state (derived, never stored) — docs/data-model.md status machine

fn derive_state(status: ChainStatus, partial: bool, live: &[ChangeStatus]) -> &'static str {
    match status {
        ChainStatus::Merged => "merged",
        ChainStatus::Abandoned => "abandoned",
        ChainStatus::Active => {
            if live.is_empty() {
                return "agents_turn"; // empty chain
            }
            if live
                .iter()
                .any(|s| matches!(s, ChangeStatus::ChangesRequested | ChangeStatus::Commented))
            {
                "agents_turn"
            } else if live.iter().any(|s| !matches!(s, ChangeStatus::Approved)) {
                "waiting_for_review"
            } else if partial {
                // All approved but the agent is still pushing (push
                // --partial): merging now would be premature.
                "agents_turn"
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

/// Chain JSON: the chain plus per-change summaries, live first.
///
/// # Errors
/// When reading review state from the database fails.
pub fn build_chain(
    conn: &Connection,
    public_base: &str,
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
            live.push(change.status);
        }
        summaries.push(summary);
    }
    Ok(types::Chain {
        id: chain.id,
        repo_path,
        branch: chain.branch.clone(),
        base: chain.base.clone(),
        status: chain.status.as_str().to_string(),
        state: derive_state(chain.status, chain.partial, &live).to_string(),
        partial: chain.partial,
        last_scan_error: chain.last_scan_error.clone(),
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
        counts: types::ChangeCounts {
            revisions: latest.number,
            published_comments,
            drafts,
            unresolved,
        },
    }))
}

// ---------------------------------------------------------------------------
// Comments

/// A comment row → its wire shape, anchor served verbatim. Comments are
/// pinned to their `(revision, side)`; the client decides which diff
/// range renders them (docs/api.md "Comment placement"), so the server
/// never ports an anchor onto another revision.
#[must_use]
pub fn comment_view(c: &db::Comment) -> types::Comment {
    types::Comment {
        id: c.id,
        change_id: c.change_id,
        revision: c.revision_number,
        parent_id: c.parent_id,
        author: c.author.clone(),
        file: c.file.clone(),
        line: c.line,
        side: c.side.clone(),
        range: c.range,
        line_text: c.line_text.clone(),
        body: c.body.clone(),
        state: c.state.clone(),
        resolved: c.resolved,
        review_id: c.review_id,
        created_at: c.created_at.clone(),
        updated_at: c.updated_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Change detail

/// Change detail JSON: every revision and every comment (anchors served
/// verbatim — the client places them by diff range, docs/api.md
/// "Comment placement").
///
/// # Errors
/// When reading review state from the database fails.
pub fn build_change_detail(conn: &Connection, change: &db::Change) -> Result<types::ChangeDetail> {
    let revisions: Vec<types::Revision> = db::revisions_for_change(conn, change.id)?
        .iter()
        .map(revision_json)
        .collect();
    let comments = db::comments_for_change(conn, change.id)?
        .iter()
        .map(comment_view)
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

#[must_use]
pub fn revision_json(rev: &db::Revision) -> types::Revision {
    types::Revision {
        number: rev.number,
        commit_sha: rev.commit_sha.clone(),
        short_sha: short_sha(&rev.commit_sha),
        parent_sha: rev.parent_sha.clone(),
        message: rev.message.clone(),
        created_at: rev.created_at.clone(),
    }
}

#[must_use]
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

/// Feedback JSON (the agent side): chain state plus actionable
/// comments per live change (docs/api.md "Feedback").
///
/// # Errors
/// When reading review state from the database fails.
pub fn build_feedback(
    conn: &Connection,
    public_base: &str,
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
        live.push(change.status);

        let latest_review = db::latest_review_for_change(conn, change.id)?;
        let comments = feedback_comments(conn, &change, latest_review.as_ref())?;
        let (_, _, unresolved) = db::comment_counts(conn, change.id)?;
        changes.push(types::FeedbackChange {
            change_id: change.id,
            change_key: change.change_key.clone(),
            subject: subject_of(&latest.message),
            commit_sha: latest.commit_sha.clone(),
            revision: latest.number,
            status: change.status.as_str().to_string(),
            unresolved,
            review: latest_review.as_ref().map(|r| types::FeedbackReview {
                verdict: r.verdict.clone(),
                message: r.message.clone(),
                revision: r.revision_number,
            }),
            comments,
        });
    }
    let state = derive_state(chain.status, chain.partial, &live);
    Ok(types::Feedback {
        state: state.to_string(),
        actionable: actionable(state),
        chain: types::FeedbackChain {
            id: chain.id,
            branch: chain.branch.clone(),
            base: chain.base.clone(),
            web_url: format!("{public_base}/chains/{}", chain.id),
            partial: chain.partial,
            last_scan_error: chain.last_scan_error.clone(),
        },
        changes,
    })
}

/// Feedback comment scope: the latest review's comments, plus
/// still-unresolved published threads from earlier reviews — each thread
/// whole (root + replies), each pinned to its own revision.
fn feedback_comments(
    conn: &Connection,
    change: &db::Change,
    latest_review: Option<&db::Review>,
) -> Result<Vec<types::Comment>> {
    let all = db::comments_for_change(conn, change.id)?;
    let in_scope_root = |root: &db::Comment| -> bool {
        if root.state != "published" {
            return false;
        }
        if latest_review.is_some_and(|r| root.review_id == Some(r.id)) {
            return true;
        }
        !root.resolved
    };
    let roots: std::collections::HashSet<i64> = all
        .iter()
        .filter(|c| c.parent_id.is_none() && in_scope_root(c))
        .map(|c| c.id)
        .collect();
    Ok(all
        .iter()
        .filter(|c| {
            c.state == "published"
                && (roots.contains(&c.id) || c.parent_id.is_some_and(|p| roots.contains(&p)))
        })
        .map(comment_view)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_table() {
        let live = |spec: &[&str]| -> Vec<ChangeStatus> {
            spec.iter()
                .map(|s| ChangeStatus::parse(s).expect("status fixture should parse"))
                .collect()
        };
        assert_eq!(derive_state(ChainStatus::Merged, false, &[]), "merged");
        assert_eq!(
            derive_state(ChainStatus::Abandoned, false, &[]),
            "abandoned"
        );
        assert_eq!(derive_state(ChainStatus::Active, false, &[]), "agents_turn");
        assert_eq!(
            derive_state(ChainStatus::Active, false, &live(&["pending"])),
            "waiting_for_review"
        );
        assert_eq!(
            derive_state(
                ChainStatus::Active,
                false,
                &live(&["approved", "changes_requested"])
            ),
            "agents_turn"
        );
        assert_eq!(
            derive_state(ChainStatus::Active, false, &live(&["commented"])),
            "agents_turn"
        );
        assert_eq!(
            derive_state(ChainStatus::Active, false, &live(&["approved", "approved"])),
            "ready_to_merge"
        );
        assert_eq!(
            derive_state(ChainStatus::Active, false, &live(&["approved", "pending"])),
            "waiting_for_review"
        );
        // partial: all approved derives agents_turn (the agent is still
        // pushing), pending keeps waiting_for_review.
        assert_eq!(
            derive_state(ChainStatus::Active, true, &live(&["approved", "approved"])),
            "agents_turn"
        );
        assert_eq!(
            derive_state(ChainStatus::Active, true, &live(&["approved", "pending"])),
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
