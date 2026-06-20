//! `nit reopen` / `nit abandon` — change-lifecycle transitions.

use anyhow::Result;
use serde_json::json;

use super::client::{Client, print_json, server_url};
use super::format::ChangeTarget;

#[derive(clap::Args)]
pub struct ReopenArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct AbandonArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    /// Optional reason recorded on the abandonment.
    #[arg(long, short = 'm')]
    pub message: Option<String>,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

/// Reopen an abandoned change so a new revision may be pushed.
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn reopen(args: ReopenArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let change_id = args.target.resolve(&client)?;
    let detail = client.post(&format!("/api/changes/{change_id}/reopen"), &json!({}))?;
    print_json(&detail)
}

/// Mark a change abandoned — a reviewer/agent judgment that it is dead
/// (reversible by `nit reopen`).
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn abandon(args: AbandonArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let change_id = args.target.resolve(&client)?;
    let body = match args.message {
        Some(message) => json!({ "message": message }),
        None => json!({}),
    };
    let detail = client.post(&format!("/api/changes/{change_id}/abandon"), &body)?;
    print_json(&detail)
}
