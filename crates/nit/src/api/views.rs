//! View assembly: the fold (`crate::review`) + reviewer drafts → the wire
//! shapes of docs/api.md. Read functions take a [`Projection`] snapshot;
//! draft rows still come from the database.

use anyhow::Result;
use rusqlite::Connection;

use crate::db;
use crate::gitscan::identity::subject_of;
use crate::review::{self, ChangeProj, Entry, Projection, ThreadComment, ThreadProj};

use super::types;

#[must_use]
pub fn short_sha(sha: &str) -> String {
    sha.chars().take(12).collect()
}

// ---------------------------------------------------------------------------
// Chain + ChangeSummary

/// Chain JSON: the chain plus per-change summaries, live first.
///
/// # Errors
/// When reading drafts from the database fails.
pub fn build_chain(
    conn: &Connection,
    public_base: &str,
    proj: &Projection,
) -> Result<types::Chain> {
    let mut summaries = Vec::new();
    for change in proj.changes_ordered() {
        if let Some(summary) = change_summary(conn, proj.chain_id, change)? {
            summaries.push(summary);
        }
    }
    Ok(types::Chain {
        id: proj.chain_id,
        repo_id: proj.repo_id,
        git_dir: proj.git_dir.clone(),
        branch: proj.branch.clone(),
        base: proj.base.clone(),
        status: proj.status,
        state: review::derive_state(proj),
        partial: proj.partial,
        last_scan_error: proj.last_scan_error.clone(),
        web_url: format!("{public_base}/chains/{}", proj.chain_id),
        created_at: proj.created_at.clone(),
        updated_at: proj.updated_at().to_string(),
        changes: summaries,
    })
}

fn change_summary(
    conn: &Connection,
    chain_id: u64,
    change: &ChangeProj,
) -> Result<Option<types::ChangeSummary>> {
    let Some(latest) = change.latest_revision() else {
        return Ok(None);
    };
    let (threads, drafts, unresolved) = comment_counts(conn, chain_id, change)?;
    Ok(Some(types::ChangeSummary {
        id: change.id,
        position: change.position,
        change_key: change.change_key.clone(),
        subject: subject_of(&latest.message),
        status: change.wire_status(),
        revision: latest.number,
        last_reviewed_revision: change.last_reviewed_revision(),
        commit_sha: latest.commit_sha.clone(),
        short_sha: short_sha(&latest.commit_sha),
        counts: types::ChangeCounts {
            revisions: latest.number,
            threads,
            drafts,
            unresolved,
        },
    }))
}

/// `(published threads, drafts, unresolved threads)` for a change.
fn comment_counts(
    conn: &Connection,
    chain_id: u64,
    change: &ChangeProj,
) -> Result<(u64, u64, u64)> {
    let threads = u64::try_from(change.threads.len()).unwrap_or(u64::MAX);
    let drafts = u64::try_from(db::drafts_for_change(conn, chain_id, &change.change_key)?.len())
        .unwrap_or(u64::MAX);
    let unresolved = change.unresolved_threads();
    Ok((
        threads,
        drafts,
        u64::try_from(unresolved).unwrap_or(u64::MAX),
    ))
}

// ---------------------------------------------------------------------------
// Threads + drafts

/// A published thread → its wire shape.
#[must_use]
pub fn thread_view(t: &ThreadProj, change_id: u64) -> types::Thread {
    types::Thread {
        id: t.id,
        change_id,
        revision: t.revision,
        file: t.file.clone(),
        line: t.line,
        side: t.side,
        range: t.range,
        line_text: t.line_text.clone(),
        resolved: t.resolved,
        comments: t.comments.iter().map(thread_comment_view).collect(),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
    }
}

#[must_use]
fn thread_comment_view(c: &ThreadComment) -> types::ThreadComment {
    types::ThreadComment {
        author: c.author,
        body: c.body.clone(),
        review_id: c.review_id,
        created_at: c.created_at.clone(),
    }
}

/// A draft row → its wire shape.
#[must_use]
pub fn draft_view(d: &db::DraftRow, change_id: u64) -> types::Draft {
    types::Draft {
        id: d.id,
        change_id,
        thread_id: d.thread_id,
        revision: d.revision,
        file: d.file.clone(),
        line: d.line,
        side: d.side,
        range: d.range,
        line_text: d.line_text.clone(),
        body: d.body.clone(),
        // For a draft, `resolved` is the decision staged on its checkbox —
        // the client reads it to show the thread's pending state.
        resolved: d.resolved.unwrap_or(false),
        created_at: d.created_at.clone(),
        updated_at: d.updated_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Change detail

/// Change detail JSON: every revision, every published thread, the
/// reviewer's open drafts, and every review.
///
/// # Errors
/// When reading drafts fails.
pub fn build_change_detail(
    conn: &Connection,
    chain_id: u64,
    change: &ChangeProj,
) -> Result<types::ChangeDetail> {
    let revisions: Vec<types::Revision> = change.revisions.iter().map(revision_json).collect();
    let threads: Vec<types::Thread> = change
        .threads
        .iter()
        .map(|t| thread_view(t, change.id))
        .collect();
    let drafts: Vec<types::Draft> = db::drafts_for_change(conn, chain_id, &change.change_key)?
        .iter()
        .map(|d| draft_view(d, change.id))
        .collect();
    let reviews = change.reviews.iter().map(review_json).collect();
    let subject = change
        .latest_revision()
        .map(|r| subject_of(&r.message))
        .unwrap_or_default();
    Ok(types::ChangeDetail {
        id: change.id,
        chain_id,
        change_key: change.change_key.clone(),
        position: change.position,
        status: change.wire_status(),
        subject,
        last_reviewed_revision: change.last_reviewed_revision(),
        revisions,
        threads,
        drafts,
        reviews,
    })
}

#[must_use]
pub fn revision_json(rev: &review::RevisionProj) -> types::Revision {
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
pub fn review_json(review: &review::ReviewProj) -> types::Review {
    types::Review {
        id: review.id,
        revision: review.revision,
        verdict: review.verdict,
        message: review.message.clone(),
        created_at: review.created_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Log entries

/// A parsed log entry → its wire shape. The one-line digest is not part of
/// the API — it is a CLI display concern derived from the raw entry on
/// demand (docs/api.md `LogEntry`).
#[must_use]
pub fn log_entry_view(entry: &Entry) -> types::LogEntry {
    types::LogEntry {
        idx: entry.idx,
        kind: entry.kind,
        created_at: entry.created_at.clone(),
        payload: entry.payload.clone(),
    }
}

// ---------------------------------------------------------------------------
// Feedback (agent side)

/// Feedback JSON: chain state plus actionable comments per live change.
/// Built purely from the in-memory fold (drafts are not part of feedback).
#[must_use]
pub fn build_feedback(public_base: &str, proj: &Projection) -> types::Feedback {
    let mut changes = Vec::new();
    for change in proj.changes_ordered() {
        if change.orphaned {
            continue; // live changes only
        }
        let Some(latest) = change.latest_revision() else {
            continue;
        };
        let threads = feedback_threads(change);
        changes.push(types::FeedbackChange {
            change_id: change.id,
            change_key: change.change_key.clone(),
            subject: subject_of(&latest.message),
            commit_sha: latest.commit_sha.clone(),
            revision: latest.number,
            status: change.wire_status(),
            unresolved: u64::try_from(change.unresolved_threads()).unwrap_or(u64::MAX),
            review: change.latest_review().map(|r| types::FeedbackReview {
                verdict: r.verdict,
                message: r.message.clone(),
                revision: r.revision,
            }),
            threads,
        });
    }
    let state = review::derive_state(proj);
    types::Feedback {
        state,
        actionable: state.actionable(),
        chain: types::FeedbackChain {
            id: proj.chain_id,
            branch: proj.branch.clone(),
            base: proj.base.clone(),
            web_url: format!("{public_base}/chains/{}", proj.chain_id),
            partial: proj.partial,
            last_scan_error: proj.last_scan_error.clone(),
        },
        changes,
    }
}

/// Feedback thread scope: the latest review's threads, plus still-unresolved
/// threads from earlier reviews — each thread whole.
fn feedback_threads(change: &ChangeProj) -> Vec<types::Thread> {
    let latest_review_id = change.latest_review().map(|r| r.id);
    change
        .threads
        .iter()
        .filter(|t| {
            // The latest review touched it (created or replied), or it is
            // still open from an earlier round.
            !t.resolved
                || (latest_review_id.is_some()
                    && t.comments.iter().any(|c| c.review_id == latest_review_id))
        })
        .map(|t| thread_view(t, change.id))
        .collect()
}
