//! `nit status` — print the chain's derived state plus one line per member.

use std::collections::HashMap;

use anyhow::Result;

use nit_types::chains::Chain;
use nit_types::changes::ChangeDetail;

use super::client::{Client, Retry, ServerOpt, print_json, server_url};
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
    #[command(flatten)]
    pub server: ServerOpt,
}

/// # Errors
/// When the server can't be reached or no chain matches the current branch.
pub fn status(args: StatusArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let change_id = resolve_chain(&client, args.chain, Retry::No)?;
    let chain: Chain = client.get(&format!("/api/chains/{change_id}"))?;
    if args.oneline {
        let unresolved = member_unresolved(&client, &chain)?;
        print!("{}", chain_oneline(&chain, &unresolved));
        Ok(())
    } else {
        print_json(&chain)
    }
}

/// Unresolved-thread count per member. The chain path carries only
/// structure, so the `--oneline` digest reads each member's change
/// snapshot for its activity — cheap since the folded state is in memory
/// server-side.
///
/// # Errors
/// When a member's change snapshot can't be fetched.
fn member_unresolved(client: &Client, chain: &Chain) -> Result<HashMap<u64, u64>> {
    let mut counts = HashMap::new();
    for member in &chain.path {
        let detail: ChangeDetail = client.get(&format!("/api/changes/{}", member.change_id))?;
        let open = detail
            .threads
            .iter()
            .filter(|t| t.revision == member.revision && !t.resolved)
            .count();
        counts.insert(member.change_id, u64::try_from(open).unwrap_or(u64::MAX));
    }
    Ok(counts)
}
