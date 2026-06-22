//! `nit status` — print the chain's derived state plus one line per member.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

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
        let unresolved = member_unresolved(&client, &chain)?;
        print!("{}", chain_oneline(&chain, &unresolved));
        Ok(())
    } else {
        print_json(&chain)
    }
}

/// Unresolved-thread count per member, read from each member's change snapshot
/// (`GET /api/changes/{id}`). The chain path carries only structure, so the
/// `--oneline` digest composes the activity it shows from the snapshots — the
/// folded state is in memory, so each is a cheap read.
///
/// # Errors
/// When a member's change snapshot can't be fetched.
fn member_unresolved(client: &Client, chain: &Value) -> Result<HashMap<u64, u64>> {
    let mut counts = HashMap::new();
    let path = chain["path"].as_array().map_or(&[][..], Vec::as_slice);
    for member in path {
        let Some(change_id) = member["change_id"].as_u64() else {
            continue;
        };
        let revision = member["revision"].as_u64();
        let detail = client.get(&format!("/api/changes/{change_id}"))?;
        let open = detail["threads"].as_array().map_or(0, |threads| {
            threads
                .iter()
                .filter(|t| {
                    t["revision"].as_u64() == revision && t["resolved"].as_bool() != Some(true)
                })
                .count()
        });
        counts.insert(change_id, u64::try_from(open).unwrap_or(u64::MAX));
    }
    Ok(counts)
}
