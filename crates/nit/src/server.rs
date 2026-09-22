//! `nit serve` — the process wiring for the axum app (`nit::api`).
//!
//! Wires it to a listener and the sqlite database.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;

#[derive(clap::Args)]
pub struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:8877")]
    pub listen: SocketAddr,
    /// Default: `$XDG_DATA_HOME/nit/nit.sqlite3` when unset.
    #[arg(long)]
    pub db: Option<PathBuf>,
}

pub fn run(args: ServeArgs) -> Result<()> {
    let db_path = match args.db {
        Some(path) => path,
        None => nit::db::default_db_path()?,
    };
    if nit::api::WEB_UI.is_none() {
        tracing::info!("built without NIT_WEB_DIST, so serving the API only");
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(args.listen).await?;
            nit::api::serve_on(listener, db_path, shutdown_signal()).await
        })
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}
