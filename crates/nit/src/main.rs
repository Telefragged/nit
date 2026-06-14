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
    /// Block until log entries land beyond the cursor; prints {head, entries, feedback}
    Wait(cli::WaitArgs),
    /// Print log entries by index/range, or stream them live with --follow
    Log(cli::LogArgs),
    /// Print the current feedback JSON without blocking
    Status(cli::StatusArgs),
    /// Reply to a review comment as the agent
    Reply(cli::ReplyArgs),
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
        Cmd::Wait(args) => cli::wait(args),
        Cmd::Log(args) => cli::log(args),
        Cmd::Status(args) => cli::status(args),
        Cmd::Reply(args) => cli::reply(args),
    }
}
