use std::path::PathBuf;

use anyhow::{Result, anyhow};

use nit_types::repos::{CreateRepo, RelocateRepo, Repo, RepoList};

use super::client::{Client, ServerOpt, server_url};
use super::format::{aligned_row, column_widths};
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
            print_repos(&list);
            Ok(())
        }
        RepoCmd::Move(a) => repo_move(&a, args.server.server),
    }
}

/// One aligned line per repo: `id  git_dir  base_ref  N active`.
fn print_repos(list: &RepoList) {
    let rows: Vec<[String; 3]> = list
        .repos
        .iter()
        .map(|r| [r.id.to_string(), r.git_dir.clone(), r.base_ref.clone()])
        .collect();
    let widths = column_widths(&rows);
    for (r, cols) in list.repos.iter().zip(&rows) {
        println!(
            "{}",
            aligned_row(cols, widths, &format!("{} active", r.active_chains))
        );
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
    println!(
        "registered repo {}  {}  base={}",
        repo.id, repo.git_dir, repo.base_ref
    );
    Ok(())
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
    println!("repo {} moved  {}", updated.id, updated.git_dir);
    Ok(())
}
