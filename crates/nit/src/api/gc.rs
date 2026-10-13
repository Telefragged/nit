//! The background collection of closed changes.
//!
//! Deletes each closed change once its last log entry is older than
//! [`RETENTION`].

use std::sync::Arc;
use std::time::Duration;

use git2::Repository;
use jiff::{SignedDuration, Timestamp};

use crate::gitscan::objects;

use super::{AppState, Appends, with_conn};

/// Interval between collections.
const INTERVAL: Duration = Duration::from_hours(1);

/// How long a closed change stays after its last log entry.
const RETENTION: SignedDuration = SignedDuration::from_hours(30 * 24);

/// Collects closed changes at startup, then once each [`INTERVAL`].
///
/// No event marks the moment that a change becomes old enough, so the
/// collection polls.
pub(super) async fn run_gc_timer(state: Arc<AppState>) {
    let mut shutdown = state.shutdown_watch();
    loop {
        collect_once(&state, Timestamp::now() - RETENTION).await;
        tokio::select! {
            () = tokio::time::sleep(INTERVAL) => {}
            _ = shutdown.wait_for(|&s| s) => break,
        }
    }
}

/// Deletes every closed change whose last log entry is older than `cutoff`.
///
/// It also deletes the keep refs of each deleted change. A failure is
/// logged as a warning, never returned.
pub async fn collect_once(state: &Arc<AppState>, cutoff: Timestamp) {
    let st = state.clone();
    let collected = with_conn(state.pool(), move |conn| {
        let mut appends = Appends::begin(conn)?;
        let deleted = appends.delete_closed_changes(cutoff)?;
        appends.commit(&st)?;
        if !deleted.is_empty() {
            tracing::info!(count = deleted.len(), "collected closed changes");
        }
        // This deletes the refs only after the commit. A crash between the
        // two can leave extra refs that pin objects. It never leaves a
        // change without its objects.
        for repo_changes in deleted.chunk_by(|a, b| a.0 == b.0) {
            let Some(repo) = st
                .repo_state(repo_changes[0].0)
                .and_then(|r| Repository::open(r.git_dir()).ok())
            else {
                continue;
            };
            for &(_, number) in repo_changes {
                objects::delete_keep_refs(&repo, number);
            }
        }
        Ok(())
    })
    .await;
    if let Err(e) = collected {
        tracing::warn!("collecting closed changes failed: {}", e.message);
    }
}
