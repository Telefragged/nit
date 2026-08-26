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
use super::git::{canonical_workdir, discover_repo, head_branch, resolve_tip};

/// The environment variable Claude Code exports into every command it runs.
///
/// The tag key stays generic, so another harness passes its own id through
/// `--tag session-id=…`.
const SESSION_ID_VAR: &str = "CLAUDE_CODE_SESSION_ID";

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
    // Observed context is a convenience. A value the vocabulary rejects
    // drops here, and the push still succeeds. A path holding a control
    // character is the case that reaches it.
    [
        canonical_workdir(repo).map(|dir| ("worktree", dir)),
        args.commit
            .is_none()
            .then(|| head_branch(repo))
            .flatten()
            .map(|name| ("branch", name)),
        std::env::var(SESSION_ID_VAR)
            .ok()
            .filter(|id| !id.is_empty())
            .map(|id| ("session-id", id)),
    ]
    .into_iter()
    .flatten()
    .filter_map(|(key, value)| Tag::new(key, value).ok())
    .chain(args.tag.iter().cloned())
    .collect()
}
