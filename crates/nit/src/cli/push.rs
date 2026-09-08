//! `nit push` — register a commit for review via `POST /api/push`.
//!
//! The cwd's checked-out commit, or an explicit revision.

use anyhow::Result;
use git2::Repository;

use nit_types::changes::TagsRequest;
use nit_types::domain::Chain;
use nit_types::domain::{Tag, Tags};
use nit_types::push::{PushRequest, PushResult};

use super::client::{Client, ServerOpt, server_url};
use super::format::print_chain_digest;
use super::git::{discover_repo, resolve_tip};
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
/// canonical ref comes from the registered repo, so no base is sent. Prints
/// the resulting chain digest — every change the push registered, not just the
/// tip — so the author needs no follow-up read.
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
    let chain: Chain = client.get(&format!("/api/chains/{}", result.tip_change.change_number))?;
    tag_chain(&client, &chain, tags)?;
    print_chain_digest(&client, &chain, None)
}

/// Puts `tags` on every change the push walked.
///
/// Labelling is its own action, so this is a second call per change
/// rather than a field on the push. A change already carrying these
/// values records nothing.
fn tag_chain(client: &Client, chain: &Chain, tags: Tags) -> Result<()> {
    if tags.is_empty() {
        return Ok(());
    }
    let body = TagsRequest { tags };
    for member in &chain.path {
        let _: serde_json::Value = client.post(
            &format!("/api/changes/{}/tags", member.change_number),
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
