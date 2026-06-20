//! `nit status` — print the chain's derived state plus one line per member.

use anyhow::Result;

use super::client::{Client, Retry, print_json, server_url};
use super::format::chain_oneline;
use super::resolve::resolve_chain;

#[derive(clap::Args)]
pub struct StatusArgs {
    /// Chain to read, by its tip change id; overrides the cwd lookup.
    #[arg(long)]
    pub chain: Option<u64>,
    /// Print a compact one-line-per-change digest instead of full JSON
    #[arg(long)]
    pub oneline: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

/// Print the chain's status: the derived state plus one line per member.
///
/// # Errors
/// When the server can't be reached or no chain matches the current branch.
pub fn status(args: StatusArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let change_id = resolve_chain(&client, args.chain, Retry::No)?;
    let chain = client.get(&format!("/api/chains/{change_id}"))?;
    if args.oneline {
        print!("{}", chain_oneline(&chain));
        Ok(())
    } else {
        print_json(&chain)
    }
}
