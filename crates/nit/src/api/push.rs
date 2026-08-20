use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;

use nit_types::domain::ChangeNumber;
use nit_types::domain::RevisionNumber;
use nit_types::domain::{LogPayload, RevisionPayload};
use nit_types::push::{PushRequest, PushResult, TipChange};

use crate::db;
use crate::gitscan;
use nit_types::domain::Lifecycle;

use super::{AppJson, AppState, ChangeEntry, Error, append_to_change, with_conn};
use super::{canonical_git_dir, map_busy};

/// Bridges push pre-flight into the append phase.
struct Target {
    entry: Arc<ChangeEntry>,
    change_number: ChangeNumber,
}

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

        // Pre-flight: reject abandoned-change pushes before writing any revisions.
        let mut targets = Vec::with_capacity(walk.commits.len());
        for wc in &walk.commits {
            let change_number = db::upsert_change(conn, repo_row.id, &wc.change_id)?;
            let entry = state
                .change(conn, change_number)?
                .ok_or_else(|| Error::internal("change vanished after upsert"))?;
            let proj = entry.read();
            let moves = proj
                .latest_revision()
                .is_none_or(|r| r.commit_sha != wc.commit_sha);
            if moves && matches!(proj.lifecycle, Lifecycle::Abandoned) {
                return Err(Error::conflict(format!(
                    "change {} is abandoned — run `nit reopen` before pushing a new revision",
                    wc.change_id
                )));
            }
            // A Change-Id is never reused: without this gate a new revision
            // would paint the merged overlay onto unreviewed content.
            if moves && proj.is_merged() {
                return Err(Error::conflict(format!(
                    "change {} is merged — new work needs its own Change-Id",
                    wc.change_id
                )));
            }
            drop(proj);
            targets.push(Target {
                entry,
                change_number,
            });
        }

        for (wc, t) in walk.commits.iter().zip(&targets) {
            let prior = t.entry.read().latest_revision().cloned();
            if prior
                .as_ref()
                .is_some_and(|r| r.commit_sha == wc.commit_sha)
            {
                continue;
            }
            let resets_status = match &prior {
                Some(old) => !gitscan::pure_rebase(
                    &repo,
                    &old.commit_sha,
                    &old.message,
                    &wc.commit_sha,
                    &wc.message,
                ),
                None => true,
            };
            let new = LogPayload::Revision(RevisionPayload {
                commit_sha: wc.commit_sha.clone(),
                parent_sha: wc.parent_sha.clone(),
                fork_sha: walk.fork_sha.clone(),
                message: wc.message.clone(),
                resets_status,
            });
            append_to_change(&state, conn, &t.entry, t.change_number, vec![new])
                .map_err(map_busy)?;
            gitscan::maintain_keep_refs(&repo, &t.entry.read());
        }

        let tip = targets
            .last()
            .expect("the empty-walk guard guarantees at least one target");
        let tip_change = {
            let proj = tip.entry.read();
            TipChange {
                change_number: tip.change_number,
                change_id: proj.change_id.clone(),
                revision: proj
                    .latest_revision()
                    .map_or(RevisionNumber::new(0), |r| r.number),
                status: proj.current_status(),
            }
        };
        Ok(Json(PushResult { tip_change }))
    })
    .await
}
