#![deny(clippy::unwrap_used)]

mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use nit::cli;

#[derive(Parser)]
#[command(
    name = "nit",
    about = "Commit-level code review for AI coding agents",
    arg_required_else_help = true
)]
struct Args {
    /// Print the client and server build versions; exit non-zero if the server
    /// is unreachable. The canonical "is nit up / installed" check.
    #[arg(short = 'V', long)]
    version: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the review server and web UI
    Serve(server::ServeArgs),
    /// Push the cwd's checked-out commit (or [COMMIT]) for review (idempotent)
    Push(cli::PushArgs),
    /// Block until log entries land beyond the seq cursor; prints {cursor, entries, feedback}
    Wait(cli::WaitArgs),
    /// Print the chain's status (--oneline for a digest)
    Status(cli::StatusArgs),
    /// Print the aggregated chain log, or stream it live with --follow
    Log(cli::LogArgs),
    /// Comment on a change (--change / --change-id): open a thread or reply (--thread)
    Comment(cli::CommentArgs),
    /// Mark a change abandoned (a reviewer/agent judgment; reopen to revert)
    Abandon(cli::AbandonArgs),
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

    let args = Args::parse();
    if args.version {
        cli::version();
        return Ok(());
    }
    let Some(cmd) = args.cmd else {
        // `arg_required_else_help` shows help for a bare `nit`; with `--version`
        // handled above, nothing else reaches here.
        return Ok(());
    };
    match cmd {
        Cmd::Serve(args) => server::run(args),
        Cmd::Push(args) => cli::push(args),
        Cmd::Wait(args) => cli::wait(args),
        Cmd::Status(args) => cli::status(args),
        Cmd::Log(args) => cli::log(args),
        Cmd::Comment(args) => cli::comment(args),
        Cmd::Abandon(args) => cli::abandon(args),
        Cmd::Reopen(args) => cli::reopen(args),
        Cmd::Repo(args) => cli::repo(args),
    }
}
