//! Which changes a command reads.
//!
//! Turns the cwd, or an explicit `--tag` / `Change-Id`, into a
//! [`Selection`], and reads the selection from the server.

use anyhow::{Result, anyhow, bail};
use git2::Repository;
use serde::Serialize;

use nit_types::changes::ChangeList;
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::LogEntry;
use nit_types::domain::{Tag, Tags};
use nit_types::log::Log;
use nit_types::repos::RepoList;

use super::client::{Client, Retry};
use super::git::discover_repo;
use super::tags::selection_tag;

/// The changes a command reads: every change in `repo` that has `tags`.
pub(crate) struct Selection {
    pub repo: u64,
    pub tags: Tags,
}

impl Selection {
    /// Reads the selected changes, ascending by change number.
    pub(crate) fn changes(&self, client: &Client, retry: Retry) -> Result<Vec<ChangeProjection>> {
        let query = self.query(None);
        let list: ChangeList = client.get_retry(&format!("/api/changes?{query}"), retry)?;
        Ok(list.changes)
    }

    /// Reads the selected changes' log entries with `sequence > after`.
    pub(crate) fn log(
        &self,
        client: &Client,
        after: Option<u64>,
        retry: Retry,
    ) -> Result<Vec<LogEntry>> {
        let query = self.query(after);
        let log: Log = client.get_retry(&format!("/api/log?{query}"), retry)?;
        Ok(log.entries)
    }

    /// The query string that selects the changes: `repo={id}&tag=key=value…`,
    /// plus `after` when given.
    ///
    /// Each value is percent-encoded, because a tag value may contain a
    /// space or an ampersand.
    fn query(&self, after: Option<u64>) -> String {
        #[derive(Serialize)]
        struct Query {
            repo: u64,
            tag: Vec<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            after: Option<u64>,
        }
        let query = Query {
            repo: self.repo,
            tag: self.tags.spelled().collect(),
            after,
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
        let (repo_id, repo) = cwd_repo(client)?;
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

/// Resolves a `Change-Id` to its change number, within the cwd's repo.
pub(crate) fn resolve_change(client: &Client, change_id: &str) -> Result<ChangeNumber> {
    let (repo_id, _) = cwd_repo(client)?;
    let list: ChangeList = client.get(&format!(
        "/api/changes?repo={repo_id}&change_id={change_id}"
    ))?;
    list.changes
        .first()
        .map(|c| c.id)
        .ok_or_else(|| anyhow!("no change with Change-Id {change_id:?} in this repo"))
}

/// The cwd's repo: its id on the server, and the repo itself.
fn cwd_repo(client: &Client) -> Result<(u64, Repository)> {
    let (git_dir, repo) = discover_repo()?;
    let list: RepoList = client.get("/api/repos")?;
    let repo_id = list
        .repos
        .iter()
        .find(|r| r.git_dir == git_dir)
        .map(|r| r.id)
        .ok_or_else(|| anyhow!("repo not registered with nit — run 'nit push' first"))?;
    Ok((repo_id, repo))
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
            tagged.query(None),
            "repo=7&tag=branch%3Dtrack%2Fa+b&tag=worktree%3D%2Fw%2Fx%3Dy%26z"
        );
        let untagged = Selection {
            repo: 7,
            tags: Tags::new(),
        };
        assert_eq!(untagged.query(Some(12)), "repo=7&after=12");
    }
}
