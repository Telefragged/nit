//! `nit push` / `nit ready` — register the cwd's checked-out commit (or an
//! explicit rev) for review via `POST /api/push`.

use anyhow::Result;
use serde_json::json;

use super::client::{Client, ServerOpt, print_json, server_url};
use super::git::{discover_repo, resolve_tip};

#[derive(clap::Args)]
pub struct PushArgs {
    /// The commit to push: any rev (sha, tag, branch). Defaults to the
    /// checked-out commit (HEAD) of the cwd — a detached HEAD or tag included.
    pub commit: Option<String>,
    /// Mark the tip partial: review can start, merging cannot; sticky until
    /// `nit ready`
    #[arg(long)]
    pub partial: bool,
    #[command(flatten)]
    pub server: ServerOpt,
}

#[derive(clap::Args)]
pub struct ReadyArgs {
    /// The commit to mark ready (see `nit push`); defaults to the cwd's HEAD.
    pub commit: Option<String>,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Push the cwd's checked-out commit (or an explicit rev) for review;
/// idempotent. The repo must already be registered (`nit repo create`).
///
/// # Errors
/// When the cwd is not a git repo, the rev can't be resolved, the server is
/// unreachable, or the push is rejected (including an unregistered repo).
pub fn push(args: PushArgs) -> Result<()> {
    do_push(
        args.commit.as_deref(),
        args.server.server,
        args.partial.then_some(true),
    )
}

/// Mark the chain complete: clear the partial flag set by `nit push --partial`.
///
/// # Errors
/// Same as [`push`].
pub fn ready(args: ReadyArgs) -> Result<()> {
    do_push(args.commit.as_deref(), args.server.server, Some(false))
}

/// Shared push/ready core: resolve the cwd's repo + the commit to push, then
/// `POST /api/push`. The canonical branch comes from the registered repo, so no
/// base is sent; `partial` only when an override is given (absent leaves it
/// unchanged).
fn do_push(commit: Option<&str>, server: Option<String>, partial: Option<bool>) -> Result<()> {
    let (git_dir, repo) = discover_repo()?;
    let tip = resolve_tip(&repo, commit)?;
    let client = Client::new(server_url(server));
    let mut body = json!({"git_dir": git_dir, "tip": tip});
    if let Some(partial) = partial {
        body["partial"] = json!(partial);
    }
    let result = client.post("/api/push", &body)?;
    print_json(&result)
}
