#![deny(clippy::unwrap_used)]

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nit=info".into()),
        )
        .init();

    nit::command::run(nit::command::Args::parse())
}
