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
    /// Register/refresh the current branch for review (idempotent)
    Push(cli::PushArgs),
    /// Block until the reviewer acts; prints the feedback JSON
    Wait(cli::WaitArgs),
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
        Cmd::Wait(args) => cli::wait(args),
        Cmd::Status(args) => cli::status(args),
        Cmd::Reply(args) => cli::reply(args),
    }
}
