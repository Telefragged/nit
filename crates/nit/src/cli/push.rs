//! `nit push` — register a commit for review via `POST /api/push`.
//!
//! The cwd's checked-out commit, or an explicit revision.

use std::collections::HashSet;

use anyhow::Result;
use git2::Repository;

use nit_types::changes::TagsRequest;
use nit_types::domain::ChangeNumber;
use nit_types::domain::{Tag, Tags};
use nit_types::push::{PushRequest, PushResult, PushedChange};

use super::client::{Client, Retry, ServerOpt, server_url};
use super::format::tagged_digest;
use super::git::{discover_repo, resolve_tip};
use super::resolve::Selection;
use super::tags::observed_tags;

#[derive(clap::Args)]
pub struct PushArgs {
    /// The commit to push: any revision (sha, tag, branch). Defaults to the
    /// checked-out commit (HEAD) of the cwd — a detached HEAD or tag included.
    pub commit: Option<String>,
    /// Tag the pushed revisions, `key=value`; repeatable. Overrides a tag of
    /// the same key derived from the environment, and a key left unnamed keeps
    /// whatever value the change already carries.
    #[arg(long = "tag")]
    pub tag: Vec<Tag>,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Pushes the cwd's checked-out commit (or an explicit revision) for review.
///
/// Idempotent. The repo must already be registered (`nit repo create`). The
/// canonical ref comes from the registered repo, so no base is sent. Then
/// it prints the digest of the changes it walked, so the author needs no
/// follow-up read.
///
/// # Errors
///
/// When the cwd is not a git repo, the revision can't be resolved, the server is
/// unreachable, or the push is rejected (including an unregistered repo).
pub fn push(args: PushArgs) -> Result<()> {
    let (git_dir, repo) = discover_repo()?;
    let tip = resolve_tip(&repo, args.commit.as_deref())?;
    let tags = push_tags(&repo, &args);
    let client = Client::new(server_url(args.server.server));
    let body = PushRequest { git_dir, tip };
    let result: PushResult = client.post("/api/push", &body)?;
    tag_changes(&client, &result.changes, &tags)?;
    let selection = Selection {
        repo: result.repo,
        tags,
    };
    // The tags narrow the read, but they can also match changes an earlier
    // push tagged, so the digest keeps only what this push walked.
    let pushed: HashSet<ChangeNumber> = result.changes.iter().map(|c| c.change_number).collect();
    let mut changes = selection.changes(&client, Retry::No)?;
    changes.retain(|c| pushed.contains(&c.id));
    print!("{}", tagged_digest(&selection.tags, &changes, None));
    Ok(())
}

/// Puts `tags` on every change the push walked.
///
/// Labelling is its own action, so this is a second call per change
/// rather than a field on the push. A change already carrying these
/// values records nothing.
fn tag_changes(client: &Client, changes: &[PushedChange], tags: &Tags) -> Result<()> {
    if tags.is_empty() {
        return Ok(());
    }
    let body = TagsRequest { tags: tags.clone() };
    for change in changes {
        let _: serde_json::Value = client.post(
            &format!("/api/changes/{}/tags", change.change_number),
            &body,
        )?;
    }
    Ok(())
}

/// The tags this push puts on its changes: what it observes, then what
/// `--tag` names.
///
/// The set carries `branch` only when the push takes the checked-out
/// commit. An explicit rev may name any commit in the repo, so the
/// checked-out branch would say nothing about it.
fn push_tags(repo: &Repository, args: &PushArgs) -> Tags {
    let mut tags = observed_tags(repo, args.commit.is_none());
    tags.overlay(&args.tag.iter().cloned().collect());
    tags
}
