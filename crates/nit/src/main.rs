#![deny(clippy::unwrap_used)]

mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use nit::cli;

#[derive(Parser)]
#[command(
    name = "nit",
    version,
    about = "Commit-level code review for AI coding agents"
)]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the review server and web UI
    Serve(server::ServeArgs),
    /// Register/refresh a branch (--repo/--branch) for review (idempotent)
    Push(cli::PushArgs),
    /// Mark the chain complete: clear the partial flag and refresh (idempotent)
    Ready(cli::ReadyArgs),
    /// Print the chain's status (--oneline for a digest)
    Status(cli::StatusArgs),
    /// Print the aggregated chain log entries by position/range
    Log(cli::LogArgs),
    /// Comment on a change (--change / --change-id): open a thread or reply (--thread)
    Comment(cli::CommentArgs),
    /// Reopen an abandoned change so a new revision can be pushed
    Reopen(cli::ReopenArgs),
    /// Inspect and manage registered repositories
    Repo(cli::RepoArgs),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nit=info".into()),
        )
        .init();

    match Args::parse().cmd {
        Cmd::Serve(args) => server::run(args),
        Cmd::Push(args) => cli::push(args),
        Cmd::Ready(args) => cli::ready(args),
        Cmd::Status(args) => cli::status(args),
        Cmd::Log(args) => cli::log(args),
        Cmd::Comment(args) => cli::comment(args),
        Cmd::Reopen(args) => cli::reopen(args),
        Cmd::Repo(args) => cli::repo(args),
    }
}
