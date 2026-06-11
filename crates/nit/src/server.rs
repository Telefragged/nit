use anyhow::Result;
use axum::{Json, Router, routing::get};
use std::net::SocketAddr;

#[derive(clap::Args)]
pub struct ServeArgs {
    /// Address to listen on
    #[arg(long, default_value = "127.0.0.1:8877")]
    pub listen: SocketAddr,
}

pub fn run(args: ServeArgs) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(args))
}

async fn serve(args: ServeArgs) -> Result<()> {
    let app = Router::new().route("/api/health", get(health));
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!("listening on http://{}", args.listen);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
