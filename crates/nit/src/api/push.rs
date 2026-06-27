use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;

use nit_types::events::{NewParent, StreamMsg};
use nit_types::log::{LogPayload, RevisionPayload};
use nit_types::push::{PushRequest, PushResult, TipChange};

use crate::db;
use crate::gitscan;
use crate::review::Lifecycle;

use super::{AppJson, AppState, ChangeEntry, Error, append_to_change, with_conn};
use super::{canonical_git_dir, map_busy};

/// Bridges push pre-flight into the append phase.
struct Target {
    entry: Arc<ChangeEntry>,
    change_id: u64,
}

#[expect(
    clippy::too_many_lines,
    reason = "one push flow: resolve, walk, pre-flight, per-commit upsert+append, result"
)]
pub(super) async fn push(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<PushRequest>,
) -> Result<Json<PushResult>, Error> {
    with_conn(state.pool(), move |conn| {
        let canonical = canonical_git_dir(&req.git_dir)?;
        let repo = Repository::open(&canonical)
            .map_err(|e| Error::internal(format!("cannot open repository: {e}")))?;

        // Push takes no base parameter -- the repo's stored canonical branch is used.
        let repo_row = db::find_repo(conn, &canonical)?.ok_or_else(|| {
            Error::not_found(format!(
                "repo at {canonical} is not registered — run `nit repo create`"
            ))
        })?;
        state.ensure_repo(&repo_row);
        let base = repo_row.base_ref.clone();

        let walk = gitscan::walk_push(&canonical, &base, &req.tip).map_err(Error::bad_request)?;
        // A tip that is ancestor-or-equal of the base walks to nothing: the work
        // already landed (or you pushed the base itself). Reject it loudly rather
        // than recording nothing, so a stray push of a merged commit is a visible
        // mistake, not a silent no-op (docs/data-model.md "Push").
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
            let change_id = db::upsert_change(conn, repo_row.id, &wc.change_key)?;
            let row = db::get_change(conn, change_id)?
                .ok_or_else(|| Error::internal("change vanished after upsert"))?;
            let entry = state.ensure_change(conn, &row)?;
            let proj = entry.read();
            let moves = proj
                .latest_revision()
                .is_none_or(|r| r.commit_sha != wc.commit_sha);
            if moves && matches!(proj.lifecycle, Lifecycle::Abandoned) {
                return Err(Error::conflict(format!(
                    "change {} is abandoned — run `nit reopen` before pushing a new revision",
                    wc.change_key
                )));
            }
            drop(proj);
            targets.push(Target { entry, change_id });
        }

        // Oldest-first: targets[i - 1] is always the parent when publishing edges.
        for (i, (wc, t)) in walk.commits.iter().zip(&targets).enumerate() {
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
                base_sha: walk.fork_sha.clone(),
                message: wc.message.clone(),
                resets_status,
            });
            append_to_change(conn, &t.entry, t.change_id, vec![new]).map_err(map_busy)?;
            gitscan::maintain_keep_refs(&repo, &t.entry.read());

            // A newly established parent↔child edge tells followers to
            // re-derive (advisory — they re-derive HEAD regardless). Publish on
            // the edge's *pre-existing* endpoint, the only feed a follower can
            // already hold: a re-rooted existing change on its own feed; a
            // brand-new child stacked on an existing parent, on the parent's.
            if i > 0 {
                let parent = &targets[i - 1];
                let feed = match &prior {
                    Some(old) if old.parent_sha != wc.parent_sha => Some(&t.entry),
                    None => Some(&parent.entry),
                    _ => None,
                };
                if let Some(feed) = feed {
                    feed.publish(StreamMsg::NewParent {
                        new_parent: NewParent {
                            of: t.change_id,
                            parent: parent.change_id,
                        },
                    });
                }
            }
        }

        let tip = targets
            .last()
            .expect("the empty-walk guard guarantees at least one target");
        let tip_change = {
            let proj = tip.entry.read();
            let rev = proj.latest_revision();
            TipChange {
                change_id: tip.change_id,
                change_key: proj.change_key.clone(),
                revision: rev.map_or(0, |r| r.number),
                status: rev.map_or(nit_types::enums::ChangeStatus::Pending, |r| {
                    proj.status_at(r.number)
                }),
            }
        };
        Ok(Json(PushResult { tip_change }))
    })
    .await
}
