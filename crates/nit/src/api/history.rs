//! The canonical-history read.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;
use serde::Deserialize;

use nit_types::graph::{HistoryCommit, RepoHistory};

use crate::db;
use crate::gitscan;

use super::{AppQuery, AppState, Error, MERGED_WINDOW, with_conn};

#[derive(Deserialize)]
pub(super) struct HistoryQuery {
    repo: u64,
}

/// Serves `GET /api/history`: a window of the canonical ref's history.
///
/// `nit_types::graph::RepoHistory` carries the walk's contract. `repo` is
/// required — a walk has no cross-repo meaning; 404 if unknown.
pub(super) async fn repo_history(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<HistoryQuery>,
) -> Result<Json<RepoHistory>, Error> {
    with_conn(state.pool(), move |conn| {
        let repo_state = state
            .repo_state(q.repo)
            .ok_or_else(|| Error::not_found(format!("no such repo: {}", q.repo)))?;
        let repo = Repository::open(repo_state.git_dir())
            .map_err(|e| Error::internal(format!("cannot open repository: {e}")))?;
        let (walked, truncated) =
            gitscan::canonical_history(&repo, &repo_state.canonical_ref, MERGED_WINDOW)
                .map_err(Error::internal)?;
        let mut commits = Vec::with_capacity(walked.len());
        for c in walked {
            let change_number = match &c.trailer {
                Some(id) => db::change_number_by_id(conn, q.repo, id)?,
                None => None,
            };
            commits.push(HistoryCommit {
                sha: c.sha,
                parents: c.parents,
                subject: c.subject,
                change_number,
                // Coupled: a trailer naming no known change nulls both.
                change_id: change_number.and(c.trailer),
            });
        }
        Ok(Json(RepoHistory { commits, truncated }))
    })
    .await
}
