//! The background lifecycle timer.
//!
//! Detects merged changes and appends `lifecycle{merged}` entries (the only
//! writer of `merged`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use git2::Repository;
use rusqlite::Connection;

use nit_types::domain::ChangeId;
use nit_types::domain::ChangeNumber;
use nit_types::domain::LifecycleAction;
use nit_types::domain::Sha;
use nit_types::log::LogPayload;

use crate::db;
use crate::gitscan;
use crate::review::ChangeProjection;
use nit_types::chain::RepoView;

use super::{AppState, append_to_change, with_conn};

/// Interval between timer sweeps, env-configurable for tests.
fn timer_interval() -> Duration {
    Duration::from_millis(
        std::env::var("NIT_TIMER_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5_000),
    )
}

/// The background sweep for **merged** changes.
///
/// Detects a change merged onto the canonical ref and appends
/// `lifecycle{merged}` entries. The only writer of `merged`. It never
/// abandons — abandonment is an explicit action (`abandon_change`).
pub(super) async fn run_lifecycle_timer(state: Arc<AppState>) {
    let interval = timer_interval();
    let mut shutdown = state.shutdown_watch();
    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => {}
            _ = shutdown.wait_for(|&s| s) => break,
        }
        sweep_once(&state).await;
    }
}

/// Runs one lifecycle sweep to completion.
///
/// The background timer calls this on its interval; tests call it directly
/// to drive merge detection without waiting on — or coupling to — the timer.
pub async fn sweep_once(state: &Arc<AppState>) {
    let st = state.clone();
    let _ = with_conn(state.pool(), move |conn| {
        sweep_lifecycle(&st, conn);
        Ok(())
    })
    .await;
}

/// One sweep of every repo whose canonical ref has moved.
///
/// For each such repo — moved since the last sweep — scan only the new
/// commits for landings, append `lifecycle{merged}`, and record the new HEAD
/// as the baseline. The baseline lives only in the DB (`repos.canonical_head`) —
/// the single source of truth — so a repo whose branch is unchanged costs
/// one ref resolution and one indexed row read.
fn sweep_lifecycle(state: &Arc<AppState>, conn: &mut Connection) {
    for repo_id in state.repo_ids() {
        let Some(repo_state) = state.repo_state(repo_id) else {
            continue;
        };
        let Ok(repo) = Repository::open(repo_state.git_dir()) else {
            continue;
        };
        let Some(head) = gitscan::resolve_head(&repo, &repo_state.canonical_ref) else {
            continue;
        };
        let recorded = db::get_repo(conn, repo_id)
            .ok()
            .flatten()
            .and_then(|r| r.canonical_head);
        if recorded.as_ref() == Some(&head) {
            continue;
        }
        // First observation has no baseline -- no landings are detected;
        // landings that predate tracking are not this timer's concern.
        if let Some(since) = &recorded {
            let Ok(view) = state.repo_view(conn, repo_id) else {
                continue;
            };
            let open = open_changes_by_key(&view);
            for (change_id, sha) in gitscan::detect_merges(&repo, since, &head, &open) {
                record_landing(state, conn, change_id, sha);
            }
        }
        // Record the baseline last: a crash before this re-scans the same delta
        // next time, which is harmless — a change merged above is terminal, so
        // it has already dropped out of the open set.
        if let Err(e) = db::update_repo_canonical_head(conn, repo_id, &head) {
            tracing::warn!(repo_id, "recording base head failed: {e:#}");
        }
    }
}

/// The sweep's working set -- looked up once per new commit.
fn open_changes_by_key(view: &RepoView) -> HashMap<ChangeId, &ChangeProjection> {
    view.change_ids()
        .into_iter()
        .filter_map(|id| view.change(id))
        .filter(|c| !c.is_terminal())
        .map(|c| (c.change_key.clone(), c))
        .collect()
}

/// The merge sweep's only lifecycle write.
fn record_landing(state: &AppState, conn: &mut Connection, change_id: ChangeNumber, sha: Sha) {
    let entry = match state.change(conn, change_id) {
        Ok(Some(entry)) => entry,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(
                change_id = change_id.get(),
                "merged change failed to load: {e:#}"
            );
            return;
        }
    };
    let new = LogPayload::lifecycle(LifecycleAction::Merged, Some(sha), None);
    if let Err(e) = append_to_change(state, conn, &entry, change_id, vec![new]) {
        tracing::warn!(
            change_id = change_id.get(),
            "lifecycle append failed: {e:#}"
        );
    }
}
