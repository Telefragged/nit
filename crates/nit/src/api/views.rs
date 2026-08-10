//! View assembly: folds + chain derivation + drafts → wire shapes.
//!
//! The per-change folds (`crate::review`), chain derivation
//! (`nit_types::chain`) and reviewer drafts become the `nit_types` wire
//! shapes. Chain views take a [`RepoView`] projection plus the repo handle
//! (for query-time tip names); draft rows come from the database.

use anyhow::Result;
use rusqlite::Connection;

use nit_types::changes::{ChangeDetail, ChangeDrafts};
use nit_types::domain::ChangeNumber;
use nit_types::domain::Draft;
use nit_types::domain::DraftDecision;
use nit_types::domain::RevisionNumber;
use nit_types::domain::Sha;
use nit_types::domain::{Chain, PathEntry};

use crate::db;
use nit_types::chain::{self, PathMember, RepoView};
use nit_types::domain::ChangeProjection;

use super::Error;

/// Builds the derived `Chain` for one tip commit-sha.
///
/// The dashboard list entry, the chain page, and the push result all share
/// this one shape.
#[must_use]
pub fn build_chain(view: &RepoView, repo_id: u64, tip_sha: &Sha) -> Chain {
    let path = view.path_from_tip(tip_sha);
    let tip_change_id = path.last().map_or(ChangeNumber(0), |m| m.change_id);
    Chain {
        tip_change_id,
        repo_id,
        state: chain::derive_state(view, &path),
        path: path_entries(view, &path),
    }
}

/// One `PathEntry` per member, read at the revision the path pins.
fn path_entries(view: &RepoView, path: &[PathMember]) -> Vec<PathEntry> {
    path.iter()
        .enumerate()
        .filter_map(|(position, m)| {
            view.change(m.change_id)
                .map(|c| path_entry(c, m, u64::try_from(position).unwrap_or(u64::MAX)))
        })
        .collect()
}

fn path_entry(change: &ChangeProjection, member: &PathMember, position: u64) -> PathEntry {
    let revision = member.revision;
    PathEntry {
        change_id: change.id,
        position,
        change_key: change.change_key.clone(),
        revision,
        status: change.status_at(revision),
        subject: change.subject_at(revision),
        commit_sha: member.commit_sha.clone(),
    }
}

/// The tip whose path walks `change` at `revision`.
///
/// Else the change's own revision sha (a dangling change is its own
/// degenerate tip). Enumerates abandoned tips too (membership-inert), so
/// an abandoned change resolves to a real chain, not only the degenerate
/// fallback.
#[must_use]
pub fn tip_for(view: &RepoView, change_id: ChangeNumber, revision: RevisionNumber) -> Option<Sha> {
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

/// Resolves the `(revision, tip_sha)` a chain handler operates on.
///
/// The explicitly `requested` revision, else the change's latest. The
/// path-walking tip is found via [`tip_for`].
///
/// # Errors
///
/// 404 if the change has no revisions, or if `requested` names a revision with
/// no enclosing tip.
pub fn resolve_revision_tip(
    view: &RepoView,
    change_id: ChangeNumber,
    requested: Option<RevisionNumber>,
) -> Result<(RevisionNumber, Sha), Error> {
    let revision = requested
        .or_else(|| {
            view.change(change_id)
                .and_then(|c| c.latest_revision().map(|r| r.number))
        })
        .ok_or_else(|| Error::not_found(format!("change {change_id} has no revisions")))?;
    let tip_sha = tip_for(view, change_id, revision)
        .ok_or_else(|| Error::not_found(format!("revision {revision} not found")))?;
    Ok((revision, tip_sha))
}

#[must_use]
pub fn draft_view(d: &db::DraftRow, change_id: ChangeNumber) -> Draft {
    Draft {
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

/// The reviewer's private overlay, read straight from the database.
///
/// Unpublished drafts and the draft decision. Not log state, so the change
/// page reads it over REST (`GET /api/changes/{id}/drafts`) while folding
/// the published projection over the websocket; the change detail folds the
/// same overlay in.
///
/// # Errors
///
/// When reading drafts fails.
pub fn change_overlay(conn: &Connection, change_id: ChangeNumber) -> Result<ChangeDrafts> {
    Ok(ChangeDrafts {
        drafts: db::drafts_for_change(conn, change_id)?
            .iter()
            .map(|d| draft_view(d, change_id))
            .collect(),
        draft_decision: db::get_draft_review(conn, change_id)?.map(|r| DraftDecision {
            decision: r.decision,
            message: r.message,
        }),
    })
}

/// A pure read of the single fold.
///
/// The chains a change sits on come from the chain endpoints
/// (`GET /api/chains/{id}`), so a change read builds no view.
///
/// # Errors
///
/// When reading drafts fails.
pub fn build_change_detail(conn: &Connection, change: &ChangeProjection) -> Result<ChangeDetail> {
    // The published view (revisions/threads/reviews) is the shared fold; the
    // reviewer's drafts and draft decision live outside the log, so overlay
    // them from the database here.
    let mut detail = nit_types::fold::change_detail(change);
    let overlay = change_overlay(conn, change.id)?;
    detail.drafts = overlay.drafts;
    detail.draft_decision = overlay.draft_decision;
    Ok(detail)
}
