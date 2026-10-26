//! `nit gc` — deletes old closed changes and unreferenced attachments.

use anyhow::Result;

use nit_types::gc::{DEFAULT_RETENTION_DAYS, GcReport, GcRequest};

use super::client::{Client, ServerOpt, server_url};

#[derive(clap::Args)]
pub struct GcArgs {
    /// The age in days, counted from its last entry, at which a closed change is deleted.
    #[arg(long, default_value_t = DEFAULT_RETENTION_DAYS)]
    pub retention_days: u16,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Requests a collection from the server, then prints what it deleted.
///
/// # Errors
///
/// When the server can't be reached or the collection fails.
pub fn gc(args: GcArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let report: GcReport = client.post(
        "/api/gc",
        &GcRequest {
            retention_days: Some(args.retention_days),
        },
    )?;
    println!("closed changes deleted: {}", report.changes);
    println!(
        "attachments deleted: {} ({})",
        report.attachments,
        humansize::format_size(report.attachment_bytes, humansize::BINARY)
    );
    Ok(())
}
