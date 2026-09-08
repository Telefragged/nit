//! `nit status` — print one line per selected change.

use anyhow::Result;

use super::client::{Client, Retry, ServerOpt, server_url};
use super::format::tagged_digest;
use super::resolve::{SelectArgs, Selection};

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
    let selection = args.select.resolve(&client)?;
    print_digest(&client, &selection, None, Retry::No)
}

/// Prints the digest of the selected changes, with a `cursor=` line first
/// when the caller gives a cursor.
///
/// # Errors
///
/// When the server can't be reached.
pub(crate) fn print_digest(
    client: &Client,
    selection: &Selection,
    cursor: Option<u64>,
    retry: Retry,
) -> Result<()> {
    let changes = selection.changes(client, retry)?;
    print!("{}", tagged_digest(&selection.tags, &changes, cursor));
    Ok(())
}
