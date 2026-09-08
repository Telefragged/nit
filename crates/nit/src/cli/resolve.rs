//! Which changes a command reads.
//!
//! Turns the cwd, or an explicit `--tag` / `Change-Id`, into a
//! [`Selection`], and reads the selection from the server.

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use nit_types::chains::ChainList;
use nit_types::changes::ChangeList;
use nit_types::domain::Chain;
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::{Tag, Tags};
use nit_types::repos::RepoList;

use super::client::{Client, Retry};
use super::git::{discover_repo, head_sha};
use super::tags::selection_tag;

/// The changes a command reads: every change in `repo` that has `tags`.
pub(crate) struct Selection {
    pub repo: u64,
    pub tags: Tags,
}

impl Selection {
    /// Reads the selected changes, ascending by change number.
    pub(crate) fn changes(&self, client: &Client, retry: Retry) -> Result<Vec<ChangeProjection>> {
        let query = self.query();
        let list: ChangeList = client.get_retry(&format!("/api/changes?{query}"), retry)?;
        Ok(list.changes)
    }

    /// The query string that selects the changes: `repo={id}&tag=key=value…`.
    ///
    /// Each value is percent-encoded, because a tag value may contain a
    /// space or an ampersand.
    fn query(&self) -> String {
        #[derive(Serialize)]
        struct Query {
            repo: u64,
            tag: Vec<String>,
        }
        let query = Query {
            repo: self.repo,
            tag: self.tags.spelled().collect(),
        };
        serde_html_form::to_string(&query).expect("a query of numbers and strings serializes")
    }
}

/// The shared `--tag` flag.
///
/// Without it, the command uses one tag from the checkout: the harness
/// session, else the worktree, else the branch HEAD points at.
#[derive(clap::Args)]
pub struct SelectArgs {
    /// Read the changes that carry this tag, `key=value`; repeatable, and
    /// every one given must match. Replaces the tag derived from the
    /// checkout (session, else worktree, else branch).
    #[arg(long = "tag")]
    pub tag: Vec<Tag>,
}

impl SelectArgs {
    /// Turns the flag, or the checkout, into a [`Selection`].
    pub(crate) fn resolve(&self, client: &Client) -> Result<Selection> {
        let (git_dir, repo) = discover_repo()?;
        let repo_id = repo_id_for(client, &git_dir, Retry::No)?;
        let tags: Tags = if self.tag.is_empty() {
            let Some(observed) = selection_tag(&repo) else {
                bail!("nothing to select by: no branch, session, or worktree — pass --tag");
            };
            std::iter::once(observed).collect()
        } else {
            self.tag.iter().cloned().collect()
        };
        Ok(Selection {
            repo: repo_id,
            tags,
        })
    }
}

/// Resolves the cwd's HEAD to its chain's tip change number.
///
/// `retry` covers only the network GETs (here and in `repo_id_for`); repo
/// discovery and a failed lookup (unregistered repo, or no chain matching
/// HEAD) stay fatal — never retried.
pub(crate) fn resolve_tip_change(client: &Client, retry: Retry) -> Result<ChangeNumber> {
    let (git_dir, repo) = discover_repo()?;
    let head = head_sha(&repo)?;
    let repo_id = repo_id_for(client, &git_dir, retry)?;
    let list: ChainList =
        client.get_retry(&format!("/api/chains?repo={repo_id}&status=all"), retry)?;
    list.chains
        .iter()
        .find(|c| c.path.last().map(|m| m.commit_sha.as_str()) == Some(head.as_str()))
        .map(|c| c.tip_change_number)
        .ok_or_else(|| anyhow!("HEAD is not registered with nit — run 'nit push' first"))
}

pub(crate) fn resolve_chain(
    client: &Client,
    explicit: Option<ChangeNumber>,
    retry: Retry,
) -> Result<ChangeNumber> {
    match explicit {
        Some(id) => Ok(id),
        None => resolve_tip_change(client, retry),
    }
}

pub(crate) fn resolve_change(client: &Client, change_id: &str) -> Result<ChangeNumber> {
    let tip = resolve_tip_change(client, Retry::No)?;
    let chain: Chain = client.get(&format!("/api/chains/{tip}"))?;
    chain
        .path
        .iter()
        .find(|m| m.change_id.as_str() == change_id)
        .map(|m| m.change_number)
        .ok_or_else(|| anyhow!("no change with Change-Id {change_id:?} on this chain"))
}

fn repo_id_for(client: &Client, git_dir: &str, retry: Retry) -> Result<u64> {
    let list: RepoList = client.get_retry("/api/repos", retry)?;
    list.repos
        .iter()
        .find(|r| r.git_dir == git_dir)
        .map(|r| r.id)
        .ok_or_else(|| anyhow!("repo not registered with nit — run 'nit push' first"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nit_types::testing::tags;

    #[test]
    fn query_repeats_and_encodes_each_tag() {
        let tagged = Selection {
            repo: 7,
            tags: tags(&[("branch", "track/a b"), ("worktree", "/w/x=y&z")]),
        };
        assert_eq!(
            tagged.query(),
            "repo=7&tag=branch%3Dtrack%2Fa+b&tag=worktree%3D%2Fw%2Fx%3Dy%26z"
        );
    }
}
