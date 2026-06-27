//! Reviews + reviewer decisions: stage/clear a draft decision and publish a
//! chain's staged decisions.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use nit_types::changes::StagedDecision;
use nit_types::decisions::{BatchSubmitResult, SubmitError};
use nit_types::enums::{Decision, LifecycleAction, Verdict};
use nit_types::log::{CommentInput, LogPayload, ReviewPayload};

use crate::db;
use crate::review::Lifecycle;

use super::{
    AppJson, AppPath, AppQuery, AppState, ChangeEntry, Error, append_to_change_with, with_conn,
};
use super::{ChainQuery, chain_context, change_or_404, map_busy};

/// One change's reviewer comment drafts as `CommentInput`s, ready to drain into
/// a `review` entry (a reply keeps its thread, a new thread carries its anchor).
fn drafts_to_comments(
    conn: &rusqlite::Connection,
    change_id: u64,
) -> anyhow::Result<Vec<CommentInput>> {
    Ok(db::drafts_for_change(conn, change_id)?
        .iter()
        .map(|d| CommentInput {
            thread_id: d.thread_id,
            revision: Some(d.revision),
            file: d.file.clone(),
            line: d.line,
            side: d.thread_id.is_none().then_some(d.side),
            range: d.range,
            line_text: d.line_text.clone(),
            body: d.body.clone(),
            resolved: d.resolved,
        })
        .collect())
}

/// Publish one reviewer `decision` for a change in **one** per-change
/// transaction (docs/data-model.md "Reviewer decisions"): a `reopen` lifecycle
/// (so a following review lands on a now-active change), then a `review` entry
/// draining the change's comment drafts (the decision's verdict, or `comment`
/// to carry staged comments when the decision is purely lifecycle), then an
/// `abandon` lifecycle — whichever the decision calls for. The drained comment
/// drafts and the change's `draft_reviews` row are deleted in the same
/// transaction, so a half-published batch never strands work and a re-submit is
/// idempotent. The shared core of `POST /reviews` and the chain batch submit;
/// callers validate the target revision/lifecycle first.
fn publish_member(
    conn: &mut rusqlite::Connection,
    state: &Arc<AppState>,
    entry: &ChangeEntry,
    change_id: u64,
    decision: Decision,
    message: &str,
    revision: u64,
) -> Result<(), Error> {
    let comments = drafts_to_comments(conn, change_id)?;
    let drained = !comments.is_empty();
    // A real verdict publishes as itself; a lifecycle-only decision still
    // publishes a `comment` review when there are comment drafts to carry.
    let verdict = decision
        .as_verdict()
        .or_else(|| drained.then_some(Verdict::Comment));

    let mut news: Vec<LogPayload> = Vec::new();
    if decision.as_lifecycle() == Some(LifecycleAction::Reopened) {
        news.push(LogPayload::lifecycle(LifecycleAction::Reopened, None, None));
    }
    if let Some(verdict) = verdict {
        news.push(LogPayload::Review(ReviewPayload {
            review_id: state.alloc_id(),
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

    append_to_change_with(conn, entry, change_id, news, |tx| {
        if drained {
            db::delete_drafts_for_change(tx, change_id)?;
        }
        db::delete_draft_review(tx, change_id)
    })
    .map_err(map_busy)?;
    Ok(())
}

/// A `lifecycle` entry (revision is set only by the merge timer).
/// `PUT /api/changes/{id}/decision` — stage (or overwrite) the change's draft
/// decision. Validated only as an enum; legality against the lifecycle is a
/// submit-time concern (a draft is reviewer scratch).
pub(super) async fn stage_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
    AppJson(req): AppJson<StagedDecision>,
) -> Result<Json<StagedDecision>, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(&state, conn, id)?;
        db::upsert_draft_review(conn, id, req.decision, &req.message)?;
        Ok(Json(req))
    })
    .await
}

/// `DELETE /api/changes/{id}/decision` — discard the staged decision (204; a
/// no-op when nothing is staged).
pub(super) async fn clear_decision(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<u64>,
) -> Result<StatusCode, Error> {
    with_conn(state.pool(), move |conn| {
        change_or_404(&state, conn, id)?;
        db::delete_draft_review(conn, id)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// `POST /api/chains/{id}/submit` — publish every chain member's staged
/// decision (docs/api.md "Chains"). Re-derives the path, then for each member
/// with a decision publishes it at the revision this path pins on the member,
/// each in its own transaction (atomic per change, not across the chain). A
/// decision illegal for the member's current lifecycle is skipped into
/// `errors` with its row kept; a published decision's row is deleted, so a
/// re-submit finishes a torn batch without double-publishing.
pub(super) async fn submit_chain(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<u64>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<BatchSubmitResult>, Error> {
    with_conn(state.pool(), move |conn| {
        let (view, _repo_id, tip_sha) = chain_context(&state, conn, change_id, q.revision)?;

        let mut submitted = 0u64;
        let mut errors = Vec::new();
        for member in view.path_from_tip(&tip_sha) {
            let Some(staged) = db::get_draft_review(conn, member.change_id)? else {
                continue; // no decision on this member — leave its comment drafts
            };
            let Some(member_entry) = state.change_entry(member.change_id) else {
                continue;
            };
            let lifecycle = member_entry.read().lifecycle;
            if let Some(reason) = decision_block(lifecycle, staged.decision) {
                errors.push(SubmitError {
                    change_id: member.change_id,
                    message: reason.to_string(),
                });
                continue;
            }
            match publish_member(
                conn,
                &state,
                &member_entry,
                member.change_id,
                staged.decision,
                &staged.message,
                member.revision,
            ) {
                Ok(()) => submitted += 1,
                Err(e) => errors.push(SubmitError {
                    change_id: member.change_id,
                    message: e.message,
                }),
            }
        }
        Ok(Json(BatchSubmitResult { submitted, errors }))
    })
    .await
}

/// Why a staged decision cannot publish against the member's current lifecycle,
/// or `None` when it is legal (an active change takes any verdict or `abandon`;
/// an abandoned change takes only `reopen`).
fn decision_block(lifecycle: Lifecycle, decision: Decision) -> Option<&'static str> {
    match (lifecycle, decision.as_lifecycle()) {
        (Lifecycle::Merged { .. }, _) => Some("change is merged — nothing to submit"),
        (Lifecycle::Abandoned, Some(LifecycleAction::Reopened)) => None,
        (Lifecycle::Abandoned, _) => Some("change is abandoned — stage Reopen first"),
        (Lifecycle::Active, Some(LifecycleAction::Reopened)) => {
            Some("change is live — Reopen does not apply")
        }
        (Lifecycle::Active, _) => None,
    }
}
