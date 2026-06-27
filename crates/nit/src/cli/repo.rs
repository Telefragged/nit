//! `nit repo` — inspect and manage the repository registry.

use std::path::PathBuf;

use anyhow::{Result, anyhow};

use crate::api::types::{CreateRepo, RelocateRepo, Repo, RepoList};

use super::client::{Client, ServerOpt, print_json, server_url};
use super::git::{discover_repo, repo_git_dir};

/// `nit repo` — inspect and manage the repository registry.
#[derive(clap::Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub cmd: RepoCmd,
    #[command(flatten)]
    pub server: ServerOpt,
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
    /// The repo's canonical base ref — any git ref, e.g. `origin/main`.
    #[arg(long)]
    pub base: String,
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
        RepoCmd::Create(a) => repo_create(&a, args.server.server),
        RepoCmd::List => {
            let client = Client::new(server_url(args.server.server));
            let list: RepoList = client.get("/api/repos")?;
            print_json(&list)
        }
        RepoCmd::Move(a) => repo_move(&a, args.server.server),
    }
}

fn repo_create(args: &RepoCreateArgs, server: Option<String>) -> Result<()> {
    let (git_dir, _repo) = discover_repo()?;
    let client = Client::new(server_url(server));
    let body = CreateRepo {
        git_dir,
        base: args.base.clone(),
    };
    let repo: Repo = client.post("/api/repos", &body)?;
    print_json(&repo)
}

fn repo_move(args: &RepoMoveArgs, server: Option<String>) -> Result<()> {
    let client = Client::new(server_url(server));
    let to = repo_git_dir(&args.to)?;
    let from = args.from.trim_end_matches('/');
    let list: RepoList = client.get("/api/repos")?;
    let id = list
        .repos
        .iter()
        .find(|r| {
            r.git_dir == from
                || r.git_dir
                    .strip_suffix("/.git")
                    .is_some_and(|root| root == from)
        })
        .map(|r| r.id)
        .ok_or_else(|| {
            anyhow!("no repo registered at '{from}' — run 'nit repo list' to see the exact paths")
        })?;
    let updated: Repo = client.patch(&format!("/api/repos/{id}"), &RelocateRepo { git_dir: to })?;
    print_json(&updated)
}
