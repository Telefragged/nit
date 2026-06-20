//! The background lifecycle timer: detect merged changes and append
//! `lifecycle{merged}` entries (the only writer of `merged`).

use std::sync::Arc;
use std::time::Duration;

use git2::Repository;

use crate::chain::RepoView;
use crate::enums::{LifecycleAction, LogKind};
use crate::gitscan;
use crate::review;

use super::{AppState, ChangeEntry, append_to_change, blocking};

/// Interval between timer sweeps, env-configurable for tests.
fn timer_interval() -> Duration {
    Duration::from_millis(
        std::env::var("NIT_TIMER_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5_000),
    )
}

/// The background sweep: detect **merged** changes (a change landed on the
/// canonical branch) and append `lifecycle{merged}` entries
/// (docs/data-model.md "Lifecycle"). The only writer of `merged`. It never
/// abandons — abandonment is an explicit action (`abandon_change`).
pub(super) async fn run_lifecycle_timer(state: Arc<AppState>) {
    let interval = timer_interval();
    let mut shutdown = state.shutdown_watch();
    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => {}
            _ = shutdown.wait_for(|&s| s) => break,
        }
        let st = state.clone();
        let _ = blocking(move || {
            sweep_lifecycle(&st);
            Ok(())
        })
        .await;
    }
}

fn sweep_lifecycle(state: &Arc<AppState>) {
    for repo_id in state.repo_ids() {
        let Some(repo_state) = state.repo_state(repo_id) else {
            continue;
        };
        let Ok(repo) = Repository::open(repo_state.git_dir()) else {
            continue;
        };
        let view = state.repo_view(repo_id);
        for change_id in live_change_ids(&view) {
            let Some(entry) = state.change_entry(change_id) else {
                continue;
            };
            let snapshot = entry.read().clone();
            if let Some(landed) =
                gitscan::landed_revision(&repo, &repo_state.base_branch, &snapshot)
            {
                append_lifecycle(
                    state,
                    &entry,
                    change_id,
                    LifecycleAction::Merged,
                    Some(landed),
                );
            }
        }
    }
}

/// Change ids in `view` that are not terminal (the timer's working set).
fn live_change_ids(view: &RepoView) -> Vec<u64> {
    view.change_ids()
        .into_iter()
        .filter(|id| view.change(*id).is_some_and(|c| !c.is_terminal()))
        .collect()
}

fn append_lifecycle(
    state: &Arc<AppState>,
    entry: &ChangeEntry,
    change_id: u64,
    action: LifecycleAction,
    revision: Option<u64>,
) {
    let payload = match serde_json::to_value(review::LifecyclePayload {
        action,
        revision,
        message: None,
    }) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("lifecycle payload: {e}");
            return;
        }
    };
    let Ok(mut conn) = state.open_db() else {
        return;
    };
    if let Err(e) = append_to_change(
        &mut conn,
        entry,
        change_id,
        vec![(LogKind::Lifecycle, payload)],
    ) {
        tracing::warn!(change_id, "lifecycle append failed: {e:#}");
    }
}
