//! Reviews + reviewer decisions.
//!
//! Set or clear a draft decision and publish the draft decisions of the
//! changes a query picks.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use nit_types::changes::ChangeQuery;
use nit_types::decisions::{BatchSubmitResult, SubmitError};
use nit_types::domain::ChangeNumber;
use nit_types::domain::DraftDecision;
use nit_types::domain::{CommentInput, LogPayload, ReviewPayload};
use nit_types::domain::{Decision, LifecycleAction, Verdict};

use crate::db;
use nit_types::domain::Lifecycle;

use super::change_or_404;
use super::{AppJson, AppPath, AppQuery, AppState, Error, append, with_conn};

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

/// Publishes the draft decision of a change.
///
/// Returns `true` when the change had a draft decision.
///
/// The function appends these entries in **one** transaction for the
/// change, in this order:
///
/// - A `reopen` lifecycle entry for a reopen decision. The review that
///   follows then applies to an active change.
/// - A `review` entry that contains the comment drafts of the change. Its
///   verdict is the verdict of the decision. For a lifecycle decision with
///   comment drafts, the verdict is `comment`.
/// - An `abandon` lifecycle entry for an abandon decision.
///
/// The review targets the latest revision. The same transaction deletes
/// the published comment drafts and the `draft_reviews` row of the change.
/// Thus a failed batch keeps every draft of the failed change, and a second
/// submit is idempotent.
///
/// # Errors
///
/// 409 when `decision_block` rejects the decision for the lifecycle of the
/// change. Also a database failure.
fn publish_change(
    conn: &mut rusqlite::Connection,
    state: &AppState,
    change_number: ChangeNumber,
) -> Result<bool, Error> {
    let appended = append(state, conn, change_number, |change, tx| {
        let Some(draft) = db::get_draft_review(tx, change_number)? else {
            return Ok(Vec::new());
        };
        let decision = draft.decision;
        if let Some(reason) = decision_block(change.lifecycle, decision) {
            return Err(Error::conflict(reason));
        }
        let comments = drafts_to_comments(tx, change_number)?;
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
                revision: change.latest_revision_number(),
                verdict,
                // A verdict decision stores the cover message on the review.
                // For a lifecycle decision, the message is the abandon reason.
                // Thus the `comment` review that contains the drafts gets an
                // empty message.
                message: if decision.as_verdict().is_some() {
                    draft.message.clone()
                } else {
                    String::new()
                },
                comments,
            }));
        }
        if decision.as_lifecycle() == Some(LifecycleAction::Abandoned) {
            let reason = (!draft.message.trim().is_empty()).then_some(draft.message);
            news.push(LogPayload::lifecycle(
                LifecycleAction::Abandoned,
                None,
                reason,
            ));
        }
        if drained {
            db::delete_drafts_for_change(tx, change_number)?;
        }
        db::delete_draft_review(tx, change_number)?;
        Ok(news)
    })?;
    // The closure appends at least one entry for every draft decision.
    Ok(!appended.entries.is_empty())
}

/// `PUT /api/changes/{id}/decision` — sets the draft decision.
///
/// Overwrites the change's draft decision when there is one. Validated
/// only as an enum; legality against the lifecycle is a submit-time concern
/// (a draft is reviewer scratch).
#[utoipa::path(put, path = "/api/changes/{id}/decision", params(("id" = ChangeNumber, Path)), request_body = DraftDecision, responses((status = 200, body = DraftDecision)))]
pub(super) async fn set_draft_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<DraftDecision>,
) -> Result<Json<DraftDecision>, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(conn, id)?;
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
#[utoipa::path(delete, path = "/api/changes/{id}/decision", params(("id" = ChangeNumber, Path)), responses((status = 204)))]
pub(super) async fn clear_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
) -> Result<StatusCode, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(conn, id)?;
        db::write(conn, |tx| db::delete_draft_review(tx, id))?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// `POST /api/submit` — publishes every draft decision the query picks.
///
/// `nit_types::decisions::BatchSubmitResult` carries the contract. Each
/// change publishes at its latest revision, in its own transaction
/// (atomic per change, not across the batch). A decision illegal for the
/// change's current lifecycle is skipped into `errors` with its row kept;
/// a published decision's row is deleted, so a re-submit finishes a torn
/// batch without double-publishing.
#[utoipa::path(post, path = "/api/submit", params(ChangeQuery), responses((status = 200, body = BatchSubmitResult)))]
pub(super) async fn submit(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ChangeQuery>,
) -> Result<Json<BatchSubmitResult>, Error> {
    with_conn(state.pool(), move |conn| {
        let repo_ids = state.repo_ids_matching(q.repo);
        let filter = db::ChangeFilter::from(q);
        let mut submitted = 0u64;
        let mut errors = Vec::new();
        for repo_id in repo_ids {
            for row in db::repo_changes(conn, repo_id, &filter)? {
                // `publish_change` opens a write transaction, so the loop calls it
                // for a change that has a draft decision.
                if db::get_draft_review(conn, row.id)?.is_none() {
                    continue; // leave its comment drafts
                }
                match publish_change(conn, &state, row.id) {
                    Ok(true) => submitted += 1,
                    Ok(false) => {}
                    Err(e) => errors.push(SubmitError {
                        change_number: row.id,
                        message: e.message,
                    }),
                }
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
