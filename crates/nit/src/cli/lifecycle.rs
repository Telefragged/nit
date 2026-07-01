//! `nit reopen` / `nit abandon` — change-lifecycle transitions.

use anyhow::Result;

use nit_types::changes::{AbandonRequest, ChangeDetail};

use super::client::{Client, ServerOpt, server_url};
use super::format::{ChangeTarget, short_key};

#[derive(clap::Args)]
pub struct ReopenArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    #[command(flatten)]
    pub server: ServerOpt,
}

#[derive(clap::Args)]
pub struct AbandonArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    /// Optional reason recorded on the abandonment.
    #[arg(long, short = 'm')]
    pub message: Option<String>,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Reopen an abandoned change so a new revision may be pushed.
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn reopen(args: ReopenArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let change_id = args.target.resolve(&client)?;
    // The server only reads the path id; no request body needed.
    let detail: ChangeDetail = client.post(&format!("/api/changes/{change_id}/reopen"), &())?;
    println!(
        "change {} {} reopened",
        detail.id,
        short_key(&detail.change_key)
    );
    Ok(())
}

/// Mark a change abandoned — a reviewer/agent judgment that it is dead
/// (reversible by `nit reopen`).
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn abandon(args: AbandonArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let change_id = args.target.resolve(&client)?;
    let body = AbandonRequest {
        message: args.message,
    };
    let detail: ChangeDetail = client.post(&format!("/api/changes/{change_id}/abandon"), &body)?;
    println!(
        "change {} {} abandoned",
        detail.id,
        short_key(&detail.change_key)
    );
    Ok(())
}
