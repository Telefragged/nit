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

/// Serves the API and the web UI until Ctrl-C.
///
/// On Linux, an update that replaces the binary restarts the server on
/// the new build.
///
/// # Errors
///
/// When the database cannot be opened or the listener cannot bind.
pub fn run(args: ServeArgs) -> Result<()> {
    let db_path = match args.db {
        Some(path) => path,
        None => crate::db::default_db_path()?,
    };
    if crate::api::WEB_UI.is_none() {
        tracing::info!("built without NIT_WEB_DIST, so serving the API only");
    }
    // The server works without the restart, for example when the user has
    // used up their inotify instances.
    if let Err(e) = crate::exe::exec_on_update() {
        tracing::warn!(
            "failed to set up inotify, so the server will not restart on a new build: {e:#}"
        );
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(args.listen).await?;
            crate::api::serve_on(listener, db_path, shutdown_signal()).await
        })
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}
