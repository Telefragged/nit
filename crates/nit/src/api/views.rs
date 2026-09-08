//! View assembly: folds + drafts → wire shapes.
//!
//! The per-change folds (`crate::review`) and reviewer drafts become the
//! `nit_types` wire shapes; draft rows come from the database.

use anyhow::Result;
use rusqlite::Connection;

use nit_types::changes::{ChangeDetail, ChangeDrafts};
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::Draft;
use nit_types::domain::DraftDecision;

use crate::db;

#[must_use]
pub fn draft_view(d: &db::DraftRow) -> Draft {
    Draft {
        id: d.id,
        change_number: d.change_number,
        thread_id: d.thread_id,
        revision: d.revision,
        anchor: d.anchor.clone(),
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
pub fn change_overlay(conn: &Connection, change_number: ChangeNumber) -> Result<ChangeDrafts> {
    Ok(ChangeDrafts {
        drafts: db::drafts_for_change(conn, change_number)?
            .iter()
            .map(draft_view)
            .collect(),
        draft_decision: db::get_draft_review(conn, change_number)?.map(|r| DraftDecision {
            decision: r.decision,
            message: r.message,
        }),
    })
}

/// A pure read of the single fold.
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
