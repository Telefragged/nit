//! Chain endpoints (derived, on demand) and the canonical-history read.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;
use serde::Deserialize;

use nit_types::chains::ChainList;
use nit_types::chains::ChainLog;
use nit_types::domain::Chain;
use nit_types::domain::ChangeId;
use nit_types::domain::ChangeNumber;
use nit_types::graph::{HistoryCommit, RepoHistory};

use crate::db;
use crate::gitscan;
use crate::review;

use super::views;
use super::{AppPath, AppQuery, AppState, Error, with_conn};
use super::{ChainQuery, MERGED_WINDOW, chain_context};

/// `?status=` filter: active-only (default) or all (includes terminal chains).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChainFilter {
    #[default]
    Active,
    All,
}

#[derive(Deserialize)]
pub(super) struct ListChainsQuery {
    #[serde(default)]
    status: ChainFilter,
    repo: Option<u64>,
}

pub(super) async fn list_chains(
    State(state): State<Arc<AppState>>,
    AppQuery(q): AppQuery<ListChainsQuery>,
) -> Result<Json<ChainList>, Error> {
    with_conn(state.pool(), move |conn| {
        let include_terminal = matches!(q.status, ChainFilter::All);
        let mut chains = Vec::new();
        for repo_id in state.repo_ids_matching(q.repo) {
            let view = state.repo_view(conn, repo_id)?;
            let tips = if include_terminal {
                view.all_tips()
            } else {
                view.tips()
            };
            for tip in tips {
                chains.push(views::build_chain(&view, repo_id, &tip));
            }
        }
        Ok(Json(ChainList { chains }))
    })
    .await
}

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
            let change_id = match &c.trailer {
                Some(key) => db::change_id_by_key(conn, q.repo, &ChangeId::from(key.clone()))?,
                None => None,
            };
            commits.push(HistoryCommit {
                sha: c.sha,
                parents: c.parents,
                subject: c.subject,
                change_id,
                // Coupled: a trailer naming no known change nulls both.
                change_key: change_id.and(c.trailer.map(ChangeId::from)),
            });
        }
        Ok(Json(RepoHistory { commits, truncated }))
    })
    .await
}

pub(super) async fn get_chain(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<ChangeNumber>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<Chain>, Error> {
    with_conn(state.pool(), move |conn| {
        let (view, repo_id, tip_sha) = chain_context(&state, conn, change_id, q.revision)?;
        Ok(Json(views::build_chain(&view, repo_id, &tip_sha)))
    })
    .await
}

/// The aggregated chain log: every member's entries, sorted by global `sequence`.
pub(super) async fn chain_log(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<ChangeNumber>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<ChainLog>, Error> {
    with_conn(state.pool(), move |conn| {
        let (view, _repo_id, tip_sha) = chain_context(&state, conn, change_id, q.revision)?;
        let path = view.path_from_tip(&tip_sha);
        let mut entries = Vec::new();
        for member in &path {
            for row in db::log_entries(conn, member.change_id, 0, None)? {
                entries.push(review::entry_from_row(member.change_id, &row)?);
            }
        }
        entries.sort_by_key(|e| e.sequence);
        Ok(Json(ChainLog { entries }))
    })
    .await
}
