//! `nit repo` — inspect and manage the repository registry.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde_json::json;

use super::client::{Client, print_json, server_url};
use super::git::{discover_repo, repo_git_dir};

/// `nit repo` — inspect and manage the repository registry.
#[derive(clap::Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub cmd: RepoCmd,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`).
    #[arg(long, global = true)]
    pub server: Option<String>,
}

#[derive(clap::Subcommand)]
pub enum RepoCmd {
    /// Register the cwd's repo so its commits can be pushed for review
    Create(RepoCreateArgs),
    /// List registered repos and their live-tip counts
    List,
    /// Repoint a repo at its new location after moving it on disk
    Move(RepoMoveArgs),
}

#[derive(clap::Args)]
pub struct RepoCreateArgs {
    /// The repo's canonical base branch. Auto-detected (`main`/`master`) when
    /// omitted; pass it when neither or both exist.
    #[arg(long)]
    pub base: Option<String>,
}

#[derive(clap::Args)]
pub struct RepoMoveArgs {
    /// The repo's current path, exactly as `nit repo list` prints its
    /// `git_dir` (or that path with the `/.git` dropped).
    pub from: String,
    /// The repo's new location on disk (a worktree or its `.git` dir).
    pub to: PathBuf,
}

/// `nit repo` dispatch.
///
/// # Errors
/// Per subcommand: server unreachable, the cwd not a git repo (`create`), or
/// an unresolvable path (`move`).
pub fn repo(args: RepoArgs) -> Result<()> {
    match args.cmd {
        RepoCmd::Create(a) => repo_create(&a, args.server),
        RepoCmd::List => {
            let client = Client::new(server_url(args.server));
            print_json(&client.get("/api/repos")?)
        }
        RepoCmd::Move(a) => repo_move(&a, args.server),
    }
}

fn repo_create(args: &RepoCreateArgs, server: Option<String>) -> Result<()> {
    let (git_dir, _repo) = discover_repo()?;
    let client = Client::new(server_url(server));
    let mut body = json!({"git_dir": git_dir});
    if let Some(base) = &args.base {
        body["base"] = json!(base);
    }
    print_json(&client.post("/api/repos", &body)?)
}

fn repo_move(args: &RepoMoveArgs, server: Option<String>) -> Result<()> {
    let client = Client::new(server_url(server));
    let to = repo_git_dir(&args.to)?;
    let from = args.from.trim_end_matches('/');
    let list = client.get("/api/repos")?;
    let repos = list["repos"]
        .as_array()
        .ok_or_else(|| anyhow!("malformed repo list: {list}"))?;
    let id = repos
        .iter()
        .find(|r| {
            let gd = r["git_dir"].as_str().unwrap_or("");
            gd == from || gd.strip_suffix("/.git").is_some_and(|root| root == from)
        })
        .and_then(|r| r["id"].as_u64())
        .ok_or_else(|| {
            anyhow!("no repo registered at '{from}' — run 'nit repo list' to see the exact paths")
        })?;
    let updated = client.patch(&format!("/api/repos/{id}"), &json!({"git_dir": to}))?;
    print_json(&updated)
}
