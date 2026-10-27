use std::path::PathBuf;

use anyhow::{Result, anyhow};

use nit_types::repos::{CreateRepo, RelocateRepo, Repo, RepoList, SetCanonicalRef};

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
    /// Point a repo at a new canonical ref at the same commit
    SetRef(RepoSetRefArgs),
    /// Delete a repo and all of its changes
    Remove(RepoRemoveArgs),
}

#[derive(clap::Args)]
pub struct RepoCreateArgs {
    /// The ref this repo tracks — any git ref, e.g. `origin/main`.
    #[arg(long)]
    pub canonical_ref: String,
}

#[derive(clap::Args)]
pub struct RepoMoveArgs {
    /// The repo's current path, exactly as `nit repo list` prints its
    /// `git_dir` (or that path with the `/.git` dropped).
    pub from: String,
    /// The repo's new location on disk (a worktree or its `.git` dir).
    pub to: PathBuf,
}

#[derive(clap::Args)]
pub struct RepoSetRefArgs {
    /// The repo's path, exactly as `nit repo list` prints its `git_dir`
    /// (or that path with the `/.git` dropped).
    pub path: String,
    /// The new canonical ref. It must resolve to the commit that nit last
    /// recorded for the current canonical ref.
    pub canonical_ref: String,
}

#[derive(clap::Args)]
pub struct RepoRemoveArgs {
    /// The repo's path, exactly as `nit repo list` prints its `git_dir`
    /// (or that path with the `/.git` dropped).
    pub path: String,
}

/// `nit repo` dispatch.
///
/// # Errors
///
/// Per subcommand: server unreachable, the cwd not a git repo (`create`), or
/// an unresolvable path (`move`), or a ref at another commit (`set-ref`).
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
        RepoCmd::SetRef(a) => repo_set_ref(&a, args.server.server),
        RepoCmd::Remove(a) => repo_remove(&a, args.server.server),
    }
}

/// One aligned line per repo: `id  git_dir  canonical_ref  N open`.
fn print_repos(list: &RepoList) {
    let rows: Vec<[String; 3]> = list
        .repos
        .iter()
        .map(|r| [r.id.to_string(), r.git_dir.clone(), r.canonical_ref.clone()])
        .collect();
    let widths = column_widths(&rows);
    for (r, cols) in list.repos.iter().zip(&rows) {
        println!(
            "{}",
            aligned_row(cols, widths, &format!("{} open", r.open_changes))
        );
    }
}

fn repo_create(args: &RepoCreateArgs, server: Option<String>) -> Result<()> {
    let (git_dir, _repo) = discover_repo()?;
    let client = Client::new(server_url(server));
    let body = CreateRepo {
        git_dir,
        canonical_ref: args.canonical_ref.clone(),
    };
    let repo: Repo = client.post("/api/repos", &body)?;
    println!(
        "registered repo {}  {}  canonical_ref={}",
        repo.id, repo.git_dir, repo.canonical_ref
    );
    Ok(())
}

fn repo_move(args: &RepoMoveArgs, server: Option<String>) -> Result<()> {
    let client = Client::new(server_url(server));
    let to = repo_git_dir(&args.to)?;
    let id = registered_repo_id(&client, &args.from)?;
    let updated: Repo = client.patch(&format!("/api/repos/{id}"), &RelocateRepo { git_dir: to })?;
    println!("repo {} moved  {}", updated.id, updated.git_dir);
    Ok(())
}

fn repo_set_ref(args: &RepoSetRefArgs, server: Option<String>) -> Result<()> {
    let client = Client::new(server_url(server));
    let id = registered_repo_id(&client, &args.path)?;
    let body = SetCanonicalRef {
        canonical_ref: args.canonical_ref.clone(),
    };
    let updated: Repo = client.put(&format!("/api/repos/{id}/canonical-ref"), &body)?;
    println!(
        "repo {}  canonical_ref={}",
        updated.id, updated.canonical_ref
    );
    Ok(())
}

fn repo_remove(args: &RepoRemoveArgs, server: Option<String>) -> Result<()> {
    let client = Client::new(server_url(server));
    let id = registered_repo_id(&client, &args.path)?;
    client.delete(&format!("/api/repos/{id}"))?;
    println!("repo {id} removed");
    Ok(())
}

/// Returns the id of the repo registered at `path`.
fn registered_repo_id(client: &Client, path: &str) -> Result<u64> {
    let path = path.trim_end_matches('/');
    let list: RepoList = client.get("/api/repos")?;
    list.repos
        .iter()
        .find(|r| {
            r.git_dir == path
                || r.git_dir
                    .strip_suffix("/.git")
                    .is_some_and(|root| root == path)
        })
        .map(|r| r.id)
        .ok_or_else(|| {
            anyhow!("no repo registered at '{path}' — run 'nit repo list' to see the exact paths")
        })
}
