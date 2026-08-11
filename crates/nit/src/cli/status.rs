//! `nit status` — print the chain's derived state plus one line per member.

use anyhow::Result;

use nit_types::domain::Chain;
use nit_types::domain::ChangeNumber;

use super::client::{Client, Retry, ServerOpt, server_url};
use super::format::print_chain_digest;
use super::resolve::resolve_chain;

#[derive(clap::Args)]
pub struct StatusArgs {
    /// Chain to read, by its tip change number; overrides the cwd lookup.
    #[arg(long)]
    pub chain: Option<ChangeNumber>,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Prints the chain's derived state plus one line per member.
///
/// # Errors
///
/// When the server can't be reached or no chain matches the current branch.
pub fn status(args: StatusArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let change_number = resolve_chain(&client, args.chain, Retry::No)?;
    let chain: Chain = client.get(&format!("/api/chains/{change_number}"))?;
    print_chain_digest(&client, &chain, None)
}
