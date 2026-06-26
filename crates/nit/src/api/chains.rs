//! Chain endpoints (derived, on demand): list, graph, fetch, and log.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use git2::Repository;
use serde::Deserialize;

use crate::db;
use crate::review;

use super::types;
use super::views;
use super::{AppPath, AppQuery, AppState, Error, with_conn};
use super::{ChainQuery, MERGED_WINDOW, change_or_404};

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
) -> Result<Json<types::ChainList>, Error> {
    tokio::task::spawn_blocking(move || {
        let include_terminal = matches!(q.status, ChainFilter::All);
        let mut chains = Vec::new();
        for repo_id in state.repo_ids() {
            if q.repo.is_some_and(|r| r != repo_id) {
                continue;
            }
            let view = state.repo_view(repo_id);
            let tips = if include_terminal {
                view.all_tips()
            } else {
                view.tips()
            };
            for tip in tips {
                chains.push(views::build_chain(&view, repo_id, &tip));
            }
        }
        Json(types::ChainList { chains })
    })
    .await
    .map_err(|e| Error::internal(format!("chain list task panicked: {e}")))
}

/// The repo's spine-centered change graph (docs/api.md "Graph").
pub(super) async fn repo_graph(
    State(state): State<Arc<AppState>>,
    AppPath(repo_id): AppPath<u64>,
) -> Result<Json<types::RepoGraph>, Error> {
    tokio::task::spawn_blocking(move || {
        let repo_state = state
            .repo_state(repo_id)
            .ok_or_else(|| Error::not_found(format!("no such repo: {repo_id}")))?;
        let repo = Repository::open(repo_state.git_dir())
            .map_err(|e| Error::internal(format!("cannot open repository: {e}")))?;
        let view = state.repo_view(repo_id);
        Ok(Json(views::build_graph(
            &repo,
            &view,
            repo_id,
            &repo_state.base_branch,
            MERGED_WINDOW,
        )?))
    })
    .await
    .map_err(|e| Error::internal(format!("repo graph task panicked: {e}")))?
}

pub(super) async fn get_chain(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<u64>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<types::Chain>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, change_id)?;
        let repo_id = entry.read().repo_id;
        let view = state.repo_view(repo_id);
        let (_, tip_sha) = views::resolve_revision_tip(&view, change_id, q.revision)?;
        Ok(Json(views::build_chain(&view, repo_id, &tip_sha)))
    })
    .await
}

/// The aggregated chain log: every member's entries, sorted by global `seq`.
pub(super) async fn chain_log(
    State(state): State<Arc<AppState>>,
    AppPath(change_id): AppPath<u64>,
    AppQuery(q): AppQuery<ChainQuery>,
) -> Result<Json<types::ChainLog>, Error> {
    with_conn(state.pool(), move |conn| {
        let entry = change_or_404(&state, conn, change_id)?;
        let repo_id = entry.read().repo_id;
        let view = state.repo_view(repo_id);
        let (_, tip_sha) = views::resolve_revision_tip(&view, change_id, q.revision)?;
        let path = view.path_from_tip(&tip_sha);
        let mut entries = Vec::new();
        for member in &path {
            for row in db::log_entries(conn, member.change_id, 0, None)? {
                entries.push(views::log_entry_view(
                    member.change_id,
                    &review::Entry::from_row(&row)?,
                ));
            }
        }
        entries.sort_by_key(|e| e.seq);
        Ok(Json(types::ChainLog { entries }))
    })
    .await
}
