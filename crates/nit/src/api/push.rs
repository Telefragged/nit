use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;

use nit_types::domain::ChangeProjection;
use nit_types::domain::{LogPayload, RevisionPayload};
use nit_types::push::{PushRequest, PushResult, PushedChange};

use crate::db;
use crate::gitscan;
use nit_types::domain::Lifecycle;

use super::{AppJson, AppState, Appends, Error, canonical_git_dir, with_conn};

/// Rejects a new revision of a terminal change.
///
/// # Errors
///
/// 409 when the sha of the commit changed and the change is abandoned or
/// merged.
fn check_revisable(change: &ChangeProjection, wc: &gitscan::WalkedCommit) -> Result<(), Error> {
    let moves = change
        .latest_revision()
        .is_none_or(|r| r.commit_sha != wc.commit_sha);
    if moves && matches!(change.lifecycle, Lifecycle::Abandoned) {
        return Err(Error::conflict(format!(
            "change {} is abandoned. Run `nit reopen` before you push a new revision",
            wc.change_id
        )));
    }
    // Each Change-Id identifies a single change for its whole life. This
    // check rejects a new revision of a merged change, because the merged
    // overlay would mark the new revision as reviewed.
    if moves && change.is_merged() {
        return Err(Error::conflict(format!(
            "change {} is merged. Give the new commit a new Change-Id",
            wc.change_id
        )));
    }
    Ok(())
}

/// The revision one walked commit records, if its sha moved.
///
/// A commit whose sha held still records nothing, so a re-push of an
/// unchanged stack leaves the log alone.
fn revision_for(
    repo: &Repository,
    walk: &gitscan::PushWalk,
    wc: &gitscan::WalkedCommit,
    proj: &ChangeProjection,
) -> Option<LogPayload> {
    let prior = proj.latest_revision();
    if prior.is_some_and(|r| r.commit_sha == wc.commit_sha) {
        return None;
    }
    let resets_status = prior.is_none_or(|old| {
        !gitscan::pure_rebase(
            repo,
            &old.commit_sha,
            &old.message,
            &wc.commit_sha,
            &wc.message,
        )
    });
    Some(LogPayload::Revision(RevisionPayload {
        commit_sha: wc.commit_sha.clone(),
        parent_sha: wc.parent_sha.clone(),
        fork_sha: walk.fork_sha.clone(),
        message: wc.message.clone(),
        resets_status,
    }))
}

#[utoipa::path(post, path = "/api/push", request_body = PushRequest, responses((status = 200, body = PushResult)))]
pub(super) async fn push(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<PushRequest>,
) -> Result<Json<PushResult>, Error> {
    with_conn(state.pool(), move |conn| {
        let canonical = canonical_git_dir(&req.git_dir)?;
        let repo = Repository::open(&canonical)
            .map_err(|e| Error::internal(format!("cannot open repository: {e}")))?;

        // Push takes no base parameter -- the repo's stored canonical ref is used.
        let repo_row = db::find_repo(conn, &canonical)?.ok_or_else(|| {
            Error::not_found(format!(
                "repo at {canonical} is not registered — run `nit repo create`"
            ))
        })?;
        state.ensure_repo(&repo_row);
        let base = repo_row.canonical_ref.clone();

        let walk = gitscan::walk_push(&canonical, &base, &req.tip).map_err(Error::bad_request)?;
        // A tip that is ancestor-or-equal of the base walks to nothing: the work
        // already merged (or you pushed the base itself). Reject it loudly rather
        // than recording nothing, so a stray push of a merged commit is a visible
        // mistake, not a silent no-op.
        if walk.commits.is_empty() {
            return Err(Error::conflict(format!(
                "tip {} is already merged into '{}' — no commits to review",
                gitscan::short_sha(&walk.fork_sha),
                base
            )));
        }

        // One transaction, so a push rejected at a terminal change inserts
        // no change and appends no revision. Without it, the changes before
        // the terminal one would stay.
        let mut appends = Appends::begin(conn)?;
        let mut pushed = Vec::with_capacity(walk.commits.len());
        for wc in &walk.commits {
            let number = appends.upsert_change(repo_row.id, &wc.change_id)?;
            let appended = appends.append(number, |change, _| {
                check_revisable(change, wc)?;
                Ok(revision_for(&repo, &walk, wc, change).into_iter().collect())
            })?;
            pushed.push(appended);
        }
        appends.commit(&state)?;

        let changes = pushed
            .iter()
            .map(|appended| {
                if !appended.entries.is_empty() {
                    gitscan::maintain_keep_refs(&repo, &appended.change);
                }
                PushedChange {
                    change_number: appended.change.id,
                    change_id: appended.change.change_id.clone(),
                    revision: appended.change.latest_revision_number(),
                    status: appended.change.current_status(),
                }
            })
            .collect();
        Ok(Json(PushResult {
            repo: repo_row.id,
            changes,
        }))
    })
    .await
}
