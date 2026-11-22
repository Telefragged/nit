//! View assembly: folds + drafts → wire shapes.
//!
//! The per-change folds (`crate::review`) and reviewer drafts become the
//! `nit_types` wire shapes; draft rows come from the database.

use anyhow::Result;
use rusqlite::Connection;

use nit_types::changes::{ChangeDetail, ChangeDrafts};
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::DraftDecision;

use crate::db;

/// The reviewer's private overlay, read straight from the database.
///
/// Unpublished drafts and the draft decision, which are not log state.
///
/// # Errors
///
/// When reading drafts fails.
pub fn change_overlay(conn: &Connection, change_number: ChangeNumber) -> Result<ChangeDrafts> {
    Ok(ChangeDrafts {
        drafts: db::drafts_for_change(conn, change_number)?,
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
    let mut detail = nit_types::fold::change_detail(change);
    let overlay = change_overlay(conn, change.id)?;
    detail.drafts = overlay.drafts;
    detail.draft_decision = overlay.draft_decision;
    Ok(detail)
}
