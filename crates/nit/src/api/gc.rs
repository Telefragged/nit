//! The collection of closed changes and of unreferenced blobs.
//!
//! `POST /api/gc` deletes each closed change once its last log entry is
//! older than the request's `retention_days`, then each blob that no
//! change links to once it is older than [`ATTACHMENT_GRACE`].

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;
use jiff::{SignedDuration, Timestamp};
use nit_types::gc::{DEFAULT_RETENTION_DAYS, GcReport, GcRequest};

use crate::db;
use crate::gitscan::objects;

use super::attachments::Collected;
use super::{AppJson, AppState, Appends, Error, with_conn};

/// How long a blob that no change links to stays after its upload.
///
/// An author attaches an upload right away, so this only has to be longer
/// than the time between the two requests.
const ATTACHMENT_GRACE: SignedDuration = SignedDuration::from_hours(1);

/// `POST /api/gc` — deletes old closed changes and unreferenced blobs.
///
/// The blobs of the changes that it deletes are among the blobs that it
/// deletes, once they are older than the grace period.
#[utoipa::path(post, path = "/api/gc", request_body = GcRequest, responses((status = 200, body = GcReport)))]
pub(super) async fn gc(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<GcRequest>,
) -> Result<Json<GcReport>, Error> {
    let days = req.retention_days.unwrap_or(DEFAULT_RETENTION_DAYS);
    let now = Timestamp::now();
    let changes = collect_changes(
        &state,
        now - SignedDuration::from_hours(24 * i64::from(days)),
    )
    .await?;
    let attachments = collect_attachments(&state, now - ATTACHMENT_GRACE).await?;
    Ok(Json(GcReport {
        changes,
        attachments: attachments.files,
        attachment_bytes: attachments.bytes,
    }))
}

/// Deletes every closed change whose last log entry is older than `cutoff`.
///
/// It also deletes the keep refs of each deleted change, and returns the
/// number of deleted changes.
///
/// # Errors
///
/// On a database failure.
pub async fn collect_changes(state: &Arc<AppState>, cutoff: Timestamp) -> Result<u64, Error> {
    let st = state.clone();
    with_conn(state.pool(), move |conn| {
        let mut appends = Appends::begin(conn)?;
        let deleted = appends.delete_closed_changes(cutoff)?;
        appends.commit(&st)?;
        // This deletes the refs only after the commit. A crash between the
        // two can leave extra refs that pin objects. It never leaves a
        // change without its objects.
        for repo_changes in deleted.chunk_by(|a, b| a.0 == b.0) {
            let Some(repo) = db::get_repo(conn, repo_changes[0].0)
                .ok()
                .flatten()
                .and_then(|r| Repository::open(r.git_dir).ok())
            else {
                continue;
            };
            for &(_, number) in repo_changes {
                objects::delete_keep_refs(&repo, number);
            }
        }
        Ok(deleted.len() as u64)
    })
    .await
}

/// Deletes every blob that no change links to and that is older than
/// `cutoff`.
///
/// # Errors
///
/// When the database or the store directory fails.
pub async fn collect_attachments(
    state: &Arc<AppState>,
    cutoff: Timestamp,
) -> Result<Collected, Error> {
    Ok(state.blobs().collect(state, cutoff).await?)
}
