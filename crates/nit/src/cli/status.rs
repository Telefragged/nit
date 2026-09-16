//! `nit status` — print one line per selected change.

use anyhow::Result;

use super::client::{Client, Retry, ServerOpt, server_url};
use super::format::tagged_digest;
use super::resolve::SelectArgs;

#[derive(clap::Args)]
pub struct StatusArgs {
    #[command(flatten)]
    pub select: SelectArgs,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Prints the digest of the selected changes.
///
/// # Errors
///
/// When the server can't be reached or the checkout selects nothing.
pub fn status(args: StatusArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let selection = args.select.resolve(&client, Retry::No)?;
    let changes = selection.changes(&client, Retry::No)?;
    print!("{}", tagged_digest(&selection.tags, &changes));
    Ok(())
}
