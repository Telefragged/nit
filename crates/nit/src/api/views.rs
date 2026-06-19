//! View assembly: the per-change folds (`crate::review`) + chain derivation
//! (`crate::chain`) + reviewer drafts → the wire shapes of docs/api.md. Chain
//! views take a [`RepoView`] snapshot plus the repo handle (for query-time tip
//! names); draft rows come from the database.

use anyhow::Result;
use git2::Repository;
use rusqlite::Connection;

use crate::chain::{self, PathMember, RepoView};
use crate::db;
use crate::enums::Side;
use crate::gitscan::{self, identity::subject_of, short_sha};
use crate::review::{self, Anchor, ChangeProj, Entry, ThreadComment, ThreadProj};

use super::types;

// ---------------------------------------------------------------------------
// Chains (derived)

/// A tip's display name: a query-time branch ref, else the tip change's
/// subject (docs/data-model.md "Tips").
#[must_use]
pub fn tip_name(repo: &Repository, view: &RepoView, path: &[PathMember]) -> String {
    let Some(tip) = path.last() else {
        return "(empty)".to_string();
    };
    if let Some(name) = gitscan::tip_name(repo, &tip.commit_sha) {
        return name;
    }
    view.change(tip.change_id)
        .and_then(|c| c.revision(tip.revision))
        .map_or_else(|| short_sha(&tip.commit_sha), |r| subject_of(&r.message))
}

/// Build a chain summary from a tip commit-sha (the dashboard entry).
///
/// # Errors
/// When reading drafts from the database fails.
pub fn build_chain_summary(
    conn: &Connection,
    repo: &Repository,
    view: &RepoView,
    repo_id: u64,
    tip_sha: &str,
) -> Result<types::ChainSummary> {
    let path = view.path_from_tip(tip_sha);
    let tip_change_id = path.last().map_or(0, |m| m.change_id);
    let entries = path_entries(conn, view, &path)?;
    Ok(types::ChainSummary {
        tip_change_id,
        repo_id,
        name: tip_name(repo, view, &path),
        state: chain::derive_state(view, &path),
        partial: chain::is_partial(view, &path),
        updated_at: path_updated_at(view, &path),
        path: entries,
    })
}

/// Build the full `Chain` for one tip commit-sha (the chain page / push result).
///
/// # Errors
/// When reading drafts from the database fails.
pub fn build_chain(
    conn: &Connection,
    repo: &Repository,
    view: &RepoView,
    repo_id: u64,
    base_branch: &str,
    tip_sha: &str,
) -> Result<types::Chain> {
    let path = view.path_from_tip(tip_sha);
    let tip_change_id = path.last().map_or(0, |m| m.change_id);
    let entries = path_entries(conn, view, &path)?;
    Ok(types::Chain {
        tip_change_id,
        repo_id,
        name: tip_name(repo, view, &path),
        base_branch: base_branch.to_string(),
        state: chain::derive_state(view, &path),
        partial: chain::is_partial(view, &path),
        path: entries,
    })
}

/// The newest member `updated_at` across a path.
fn path_updated_at(view: &RepoView, path: &[PathMember]) -> String {
    path.iter()
        .filter_map(|m| view.change(m.change_id))
        .map(|c| c.updated_at().to_string())
        .max()
        .unwrap_or_default()
}

/// One `PathEntry` per member, read at the revision the path pins.
fn path_entries(
    conn: &Connection,
    view: &RepoView,
    path: &[PathMember],
) -> Result<Vec<types::PathEntry>> {
    path.iter()
        .enumerate()
        .filter_map(|(position, m)| {
            view.change(m.change_id)
                .map(|c| path_entry(conn, c, m, u64::try_from(position).unwrap_or(u64::MAX)))
        })
        .collect()
}

fn path_entry(
    conn: &Connection,
    change: &ChangeProj,
    member: &PathMember,
    position: u64,
) -> Result<types::PathEntry> {
    let revision = member.revision;
    let latest_revision = change.latest_revision().map_or(revision, |r| r.number);
    let subject = change
        .revision(revision)
        .map(|r| subject_of(&r.message))
        .unwrap_or_default();
    let drafts = u64::try_from(
        db::drafts_for_change(conn, change.id)?
            .iter()
            .filter(|d| d.revision == revision)
            .count(),
    )
    .unwrap_or(u64::MAX);
    let threads = u64::try_from(
        change
            .threads
            .iter()
            .filter(|t| t.revision == revision)
            .count(),
    )
    .unwrap_or(u64::MAX);
    Ok(types::PathEntry {
        change_id: change.id,
        position,
        change_key: change.change_key.clone(),
        revision,
        latest_revision,
        status: change.status_at(revision),
        merged_elsewhere: change.merged_elsewhere(revision),
        subject,
        commit_sha: member.commit_sha.clone(),
        counts: types::ChangeCounts {
            threads,
            drafts,
            unresolved: u64::try_from(change.unresolved_at(revision)).unwrap_or(u64::MAX),
        },
        draft_decision: db::get_draft_review(conn, change.id)?.map(|r| r.decision),
    })
}

/// The tip whose path walks `change` at `revision`, else the change's own
/// revision sha (a dangling change is its own degenerate tip). Enumerates
/// abandoned leaves too (membership-inert), so an abandoned change resolves to
/// a real chain, not only the degenerate fallback.
#[must_use]
pub fn tip_for(view: &RepoView, change_id: u64, revision: u64) -> Option<String> {
    for tip in view.enumerable_tips() {
        let path = view.path_from_tip(&tip);
        if path
            .iter()
            .any(|m| m.change_id == change_id && m.revision == revision)
        {
            return Some(tip);
        }
    }
    view.change(change_id)
        .and_then(|c| c.revision(revision))
        .map(|r| r.commit_sha.clone())
}

// ---------------------------------------------------------------------------
// Threads + drafts

/// A published thread → its wire shape, projecting its [`Anchor`] back to the
/// flat `file`/`line`/`side`/`range`/`line_text` fields.
#[must_use]
pub fn thread_view(t: &ThreadProj, change_id: u64) -> types::Thread {
    let (file, line, side, range, line_text) = match &t.anchor {
        Anchor::Change => (None, None, Side::New, None, None),
        Anchor::File { file } => (Some(file.clone()), None, Side::New, None, None),
        Anchor::Line {
            file,
            side,
            line,
            line_text,
            range,
        } => (
            Some(file.clone()),
            Some(*line),
            *side,
            *range,
            line_text.clone(),
        ),
    };
    types::Thread {
        id: t.id,
        change_id,
        revision: t.revision,
        file,
        line,
        side,
        range,
        line_text,
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
        resolved: d.resolved.unwrap_or(false),
        created_at: d.created_at.clone(),
        updated_at: d.updated_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Change detail

/// Change detail JSON: every revision, every published thread, the reviewer's
/// open drafts, every review, and the tips that walk through this change.
///
/// # Errors
/// When reading drafts fails.
pub fn build_change_detail(
    conn: &Connection,
    repo: &Repository,
    view: &RepoView,
    change: &ChangeProj,
) -> Result<types::ChangeDetail> {
    let revisions: Vec<types::Revision> = change.revisions.iter().map(revision_json).collect();
    let threads: Vec<types::Thread> = change
        .threads
        .iter()
        .map(|t| thread_view(t, change.id))
        .collect();
    let drafts: Vec<types::Draft> = db::drafts_for_change(conn, change.id)?
        .iter()
        .map(|d| draft_view(d, change.id))
        .collect();
    let reviews = change.reviews.iter().map(review_json).collect();
    let subject = change
        .latest_revision()
        .map(|r| subject_of(&r.message))
        .unwrap_or_default();
    let chains = view
        .chains_through(change.id)
        .into_iter()
        .map(|hit| types::ChainRef {
            tip_change_id: hit.tip_change_id,
            revision: hit.revision,
            name: tip_name(repo, view, &hit.path),
        })
        .collect();
    let draft_decision = db::get_draft_review(conn, change.id)?.map(|r| types::StagedDecision {
        decision: r.decision,
        message: r.message,
    });
    Ok(types::ChangeDetail {
        id: change.id,
        repo_id: change.repo_id,
        change_key: change.change_key.clone(),
        subject,
        revisions,
        threads,
        drafts,
        reviews,
        chains,
        draft_decision,
    })
}

#[must_use]
pub fn revision_json(rev: &review::RevisionProj) -> types::Revision {
    types::Revision {
        number: rev.number,
        commit_sha: rev.commit_sha.clone(),
        parent_sha: rev.parent_sha.clone(),
        base_sha: rev.base_sha.clone(),
        partial: rev.partial,
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

/// A parsed log entry → its wire shape.
#[must_use]
pub fn log_entry_view(change_id: u64, entry: &Entry) -> types::LogEntry {
    types::LogEntry {
        change_id,
        idx: entry.idx,
        seq: entry.seq,
        kind: entry.kind,
        created_at: entry.created_at.clone(),
        payload: entry.payload.clone(),
    }
}
