//! API id-resolution: map the cwd's repo + HEAD (or an explicit `--chain` /
//! `Change-Id`) to the numeric ids the server's endpoints take.

use anyhow::{Result, anyhow};

use nit_types::chains::{Chain, ChainList};
use nit_types::repos::RepoList;

use super::client::{Client, Retry};
use super::git::{discover_repo, head_sha};

/// The tip change id for the cwd's repo + branch HEAD: find the chain whose tip
/// commit-sha equals the local HEAD, via `GET /api/chains?status=all`. `retry`
/// covers only the GETs — repo discovery and "not registered" stay fatal.
pub(crate) fn resolve_tip_change(client: &Client, retry: Retry) -> Result<u64> {
    let (git_dir, repo) = discover_repo()?;
    let head = head_sha(&repo)?;
    let repo_id = repo_id_for(client, &git_dir, retry)?;
    let list: ChainList =
        client.get_retry(&format!("/api/chains?repo={repo_id}&status=all"), retry)?;
    list.chains
        .iter()
        .find(|c| c.path.last().map(|m| m.commit_sha.as_str()) == Some(head.as_str()))
        .map(|c| c.tip_change_id)
        .ok_or_else(|| anyhow!("HEAD is not registered with nit — run 'nit push' first"))
}

/// The chain's tip change id: the explicit `--chain` when given, else the
/// cwd's tip change.
pub(crate) fn resolve_chain(client: &Client, explicit: Option<u64>, retry: Retry) -> Result<u64> {
    match explicit {
        Some(id) => Ok(id),
        None => resolve_tip_change(client, retry),
    }
}

/// The numeric change id for a `Change-Id` trailer on the cwd's chain.
pub(crate) fn resolve_change(client: &Client, change_key: &str) -> Result<u64> {
    let tip = resolve_tip_change(client, Retry::No)?;
    let chain: Chain = client.get(&format!("/api/chains/{tip}"))?;
    chain
        .path
        .iter()
        .find(|m| m.change_key == change_key)
        .map(|m| m.change_id)
        .ok_or_else(|| anyhow!("no change with Change-Id {change_key:?} on this chain"))
}

fn repo_id_for(client: &Client, git_dir: &str, retry: Retry) -> Result<u64> {
    let list: RepoList = client.get_retry("/api/repos", retry)?;
    list.repos
        .iter()
        .find(|r| r.git_dir == git_dir)
        .map(|r| r.id)
        .ok_or_else(|| anyhow!("repo not registered with nit — run 'nit push' first"))
}
