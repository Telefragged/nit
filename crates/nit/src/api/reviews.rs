//! Reviews + reviewer decisions.
//!
//! Set or clear a draft decision and publish a chain's draft decisions.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use nit_types::decisions::{BatchSubmitResult, SubmitError};
use nit_types::domain::ChangeNumber;
use nit_types::domain::DraftDecision;
use nit_types::domain::RevisionNumber;
use nit_types::domain::{CommentInput, LogPayload, ReviewPayload};
use nit_types::domain::{Decision, LifecycleAction, Verdict};

use crate::db;
use nit_types::domain::Lifecycle;

use super::{
    AppJson, AppPath, AppQuery, AppState, ChangeEntry, Error, append_to_change_with, with_conn,
};
use super::{ChainQuery, chain_context, change_or_404, map_busy};

/// One change's reviewer comment drafts as `CommentInput`s.
///
/// Ready to drain into a `review` entry (a reply keeps its thread, a new
/// thread carries its anchor).
fn drafts_to_comments(
    conn: &rusqlite::Connection,
    change_number: ChangeNumber,
) -> anyhow::Result<Vec<CommentInput>> {
    Ok(db::drafts_for_change(conn, change_number)?
        .into_iter()
        .map(|d| CommentInput {
            thread_id: d.thread_id,
            revision: Some(d.revision),
            // A reply takes the anchor its thread already holds.
            anchor: d.thread_id.is_none().then_some(d.anchor),
            body: d.body,
            resolved: d.resolved,
        })
        .collect())
}

/// Publishes one reviewer `decision` for a change.
///
/// All in **one** per-change transaction: a `reopen` lifecycle (so a
/// following review lands on a now-active change), then a `review` entry
/// draining the change's comment drafts (the decision's verdict, or
/// `comment` to carry draft comments when the decision is purely
/// lifecycle), then an `abandon` lifecycle — whichever the decision calls
/// for. The drained comment drafts and the change's `draft_reviews` row are
/// deleted in the same transaction, so a half-published batch never strands
/// work and a re-submit is idempotent. Called per member by the chain batch
/// submit — the only publish path; the caller validates the target
/// revision/lifecycle first.
fn publish_member(
    conn: &mut rusqlite::Connection,
    state: &Arc<AppState>,
    entry: &ChangeEntry,
    change_number: ChangeNumber,
    decision: Decision,
    message: &str,
    revision: RevisionNumber,
) -> Result<(), Error> {
    let comments = drafts_to_comments(conn, change_number)?;
    let drained = !comments.is_empty();
    let verdict = decision
        .as_verdict()
        .or_else(|| drained.then_some(Verdict::Comment));

    let mut news: Vec<LogPayload> = Vec::new();
    if decision.as_lifecycle() == Some(LifecycleAction::Reopened) {
        news.push(LogPayload::lifecycle(LifecycleAction::Reopened, None, None));
    }
    if let Some(verdict) = verdict {
        news.push(LogPayload::Review(ReviewPayload {
            revision,
            verdict,
            // The cover message rides a real verdict; for a lifecycle decision
            // it is the abandon reason, so the carrier `comment` review has none.
            message: if decision.as_verdict().is_some() {
                message.to_string()
            } else {
                String::new()
            },
            comments,
        }));
    }
    if decision.as_lifecycle() == Some(LifecycleAction::Abandoned) {
        let reason = (!message.trim().is_empty()).then(|| message.to_string());
        news.push(LogPayload::lifecycle(
            LifecycleAction::Abandoned,
            None,
            reason,
        ));
    }

    append_to_change_with(state, conn, entry, change_number, news, |tx| {
        if drained {
            db::delete_drafts_for_change(tx, change_number)?;
        }
        db::delete_draft_review(tx, change_number)
    })
    .map_err(map_busy)?;
    Ok(())
}

/// `PUT /api/changes/{id}/decision` — sets the draft decision.
///
/// Overwrites the change's draft decision when there is one. Validated
/// only as an enum; legality against the lifecycle is a submit-time concern
/// (a draft is reviewer scratch).
pub(super) async fn set_draft_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<DraftDecision>,
) -> Result<Json<DraftDecision>, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(&state, conn, id)?;
        db::write(conn, |tx| {
            db::upsert_draft_review(tx, id, req.decision, &req.message)
        })?;
        Ok(Json(req))
    })
    .await
}

/// `DELETE /api/changes/{id}/decision` — discards the draft decision.
///
/// 204; a no-op when nothing is drafted.
pub(super) async fn clear_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
) -> Result<StatusCode, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(&state, conn, id)?;
        db::write(conn, |tx| db::delete_draft_review(tx, id))?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// `POST /api/chains/{id}/submit` — publishes every draft decision.
///
/// Re-derives the path, then for each chain member with a decision
/// publishes it at the revision this path pins on the member, each in its
/// own transaction (atomic per change, not across the chain). A decision
/// illegal for the member's current lifecycle is skipped into `errors`
/// with its row kept; a published decision's row is deleted, so a
/// re-submit finishes a torn batch without double-publishing.
pub(super) async fn submit_chain(
    State(state): State<Arc<AppState>>,
    AppPath(change_number): AppPath<ChangeNumber>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<BatchSubmitResult>, Error> {
    with_conn(state.pool(), move |conn| {
        let (view, _repo_id, tip_sha) = chain_context(&state, conn, change_number, q.revision)?;

        let mut submitted = 0u64;
        let mut errors = Vec::new();
        for member in view.path_from_tip(&tip_sha) {
            let Some(draft) = db::get_draft_review(conn, member.change_number)? else {
                continue; // leave its comment drafts
            };
            let Some(member_entry) = state.change(conn, member.change_number)? else {
                continue;
            };
            let lifecycle = member_entry.read().lifecycle;
            if let Some(reason) = decision_block(lifecycle, draft.decision) {
                errors.push(SubmitError {
                    change_number: member.change_number,
                    message: reason.to_string(),
                });
                continue;
            }
            match publish_member(
                conn,
                &state,
                &member_entry,
                member.change_number,
                draft.decision,
                &draft.message,
                member.revision,
            ) {
                Ok(()) => submitted += 1,
                Err(e) => errors.push(SubmitError {
                    change_number: member.change_number,
                    message: e.message,
                }),
            }
        }
        Ok(Json(BatchSubmitResult { submitted, errors }))
    })
    .await
}

fn decision_block(lifecycle: Lifecycle, decision: Decision) -> Option<&'static str> {
    match (lifecycle, decision.as_lifecycle()) {
        (Lifecycle::Merged, _) => Some("change is merged — nothing to submit"),
        (Lifecycle::Abandoned, Some(LifecycleAction::Reopened)) => None,
        (Lifecycle::Abandoned, _) => Some("change is abandoned — draft Reopen first"),
        (Lifecycle::Active, Some(LifecycleAction::Reopened)) => {
            Some("change is live — Reopen does not apply")
        }
        (Lifecycle::Active, _) => None,
    }
}
