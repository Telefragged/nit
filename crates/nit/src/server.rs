//! `nit serve` — wires the axum app (`nit::api`) to a listener, the
//! sqlite database and the optional built web UI.

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
    /// Built web UI directory served outside /api
    /// (default: `$NIT_WEB_DIST`; API-only when unset)
    #[arg(long)]
    pub web_dist: Option<PathBuf>,
}

pub fn run(args: ServeArgs) -> Result<()> {
    let db_path = match args.db {
        Some(path) => path,
        None => nit::db::default_db_path()?,
    };
    let web_dist = args
        .web_dist
        .or_else(|| std::env::var_os("NIT_WEB_DIST").map(PathBuf::from));
    if web_dist.is_none() {
        tracing::info!("no --web-dist/$NIT_WEB_DIST — serving the API only");
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(args.listen).await?;
            nit::api::serve_on(listener, db_path, web_dist, shutdown_signal()).await
        })
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}
