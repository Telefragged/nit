//! `nit push` / `nit ready` — register the cwd's checked-out commit (or an
//! explicit rev) for review via `POST /api/push`.

use anyhow::Result;
use serde_json::json;

use super::client::{Client, print_json, server_url};
use super::git::{discover_repo, resolve_tip};

#[derive(clap::Args)]
pub struct PushArgs {
    /// The commit to push: any rev (sha, tag, branch). Defaults to the
    /// checked-out commit (HEAD) of the cwd — a detached HEAD or tag included.
    pub commit: Option<String>,
    /// The repo's canonical base branch. Detected server-side (`main` or
    /// `master`) when omitted; pass it when neither or both exist.
    #[arg(long)]
    pub base: Option<String>,
    /// Mark the tip partial: review can start, merging cannot; sticky until
    /// `nit ready`
    #[arg(long)]
    pub partial: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct ReadyArgs {
    /// The commit to mark ready (see `nit push`); defaults to the cwd's HEAD.
    pub commit: Option<String>,
    /// The repo's canonical base branch (see `nit push`).
    #[arg(long)]
    pub base: Option<String>,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

/// Push the cwd's checked-out commit (or an explicit rev) for review;
/// idempotent.
///
/// # Errors
/// When the cwd is not a git repo, the rev can't be resolved, the server is
/// unreachable, or the push is rejected.
pub fn push(args: PushArgs) -> Result<()> {
    do_push(
        args.commit.as_deref(),
        args.base,
        args.server,
        args.partial.then_some(true),
    )
}

/// Mark the chain complete: clear the partial flag set by `nit push --partial`.
///
/// # Errors
/// Same as [`push`].
pub fn ready(args: ReadyArgs) -> Result<()> {
    do_push(args.commit.as_deref(), args.base, args.server, Some(false))
}

/// Shared push/ready core: resolve the cwd's repo + the commit to push, then
/// `POST /api/push`. `base` is sent only when given (else the server detects
/// it); `partial` only when an override is given (absent leaves it unchanged).
fn do_push(
    commit: Option<&str>,
    base: Option<String>,
    server: Option<String>,
    partial: Option<bool>,
) -> Result<()> {
    let (git_dir, repo) = discover_repo()?;
    let tip = resolve_tip(&repo, commit)?;
    let client = Client::new(server_url(server));
    let mut body = json!({"git_dir": git_dir, "tip": tip});
    if let Some(base) = base {
        body["base"] = json!(base);
    }
    if let Some(partial) = partial {
        body["partial"] = json!(partial);
    }
    let result = client.post("/api/push", &body)?;
    print_json(&result)
}
