mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    }
}
