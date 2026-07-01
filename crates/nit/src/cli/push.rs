//! `nit push` — register the cwd's checked-out commit (or an explicit rev) for
//! review via `POST /api/push`.

use anyhow::Result;

use nit_types::chains::Chain;
use nit_types::push::{PushRequest, PushResult};

use super::client::{Client, ServerOpt, server_url};
use super::format::print_chain_digest;
use super::git::{discover_repo, resolve_tip};

#[derive(clap::Args)]
pub struct PushArgs {
    /// The commit to push: any rev (sha, tag, branch). Defaults to the
    /// checked-out commit (HEAD) of the cwd — a detached HEAD or tag included.
    pub commit: Option<String>,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Push the cwd's checked-out commit (or an explicit rev) for review;
/// idempotent. The repo must already be registered (`nit repo create`). The
/// canonical branch comes from the registered repo, so no base is sent. Prints
/// the resulting chain digest — every change the push registered, not just the
/// tip — so the agent needs no follow-up read.
///
/// # Errors
/// When the cwd is not a git repo, the rev can't be resolved, the server is
/// unreachable, or the push is rejected (including an unregistered repo).
pub fn push(args: PushArgs) -> Result<()> {
    let (git_dir, repo) = discover_repo()?;
    let tip = resolve_tip(&repo, args.commit.as_deref())?;
    let client = Client::new(server_url(args.server.server));
    let body = PushRequest { git_dir, tip };
    let result: PushResult = client.post("/api/push", &body)?;
    let chain: Chain = client.get(&format!("/api/chains/{}", result.tip_change.change_id))?;
    print_chain_digest(&client, &chain, None)
}
