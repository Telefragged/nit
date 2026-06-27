//! `nit wait` — block until the chain's aggregated log holds something worth
//! acting on past the `seq` cursor, parking the websocket as a doorbell.

use anyhow::Result;
use serde::Serialize;

use nit_types::chains::Chain;
use nit_types::log::{ChainLog, LogEntry};

use super::client::{Client, Retry, ServerOpt, next_text, print_json, server_url};
use super::format::print_oneline_entries;
use super::log::{heads, max_seq};
use super::resolve::resolve_tip_change;

#[derive(clap::Args)]
pub struct WaitArgs {
    /// Global `seq` cursor: the highest log seq already consumed (start at 0,
    /// then pass the `cursor` each result prints; docs/agent-workflow.md).
    pub cursor: u64,
    /// Print a one-line digest per entry instead of full payloads
    #[arg(long)]
    pub oneline: bool,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// The `nit wait` result: the entries drained past the cursor plus the chain
/// snapshot the agent acts on.
#[derive(Serialize)]
struct WaitResult {
    cursor: u64,
    entries: Vec<LogEntry>,
    feedback: Chain,
}

/// Block until the chain's aggregated log holds something worth acting on past
/// the `seq` cursor, then print `{cursor, entries, feedback}`. No timeout — the
/// agent calls this only when it has nothing else to do.
///
/// Each pass **drains `(cursor, head]` from the log** (the log is the source of
/// truth) and returns it if non-empty; otherwise it blocks the websocket as a
/// doorbell — until any new entry lands, then re-drains. Reading from
/// the log rather than the stream is what makes a single `wait` surface *every*
/// entry since the cursor, not just the first. Rides out server restarts.
///
/// # Errors
/// When the server returns a malformed response or a fatal client error.
pub fn wait(args: WaitArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let retry = Retry::UntilUp;
    let mut cursor = args.cursor;
    // HEAD is fixed for this command's lifetime, so the tip change id (the
    // chain's stable identity) resolves once, not every loop pass.
    let tip = resolve_tip_change(&client, retry)?;
    loop {
        let log: ChainLog = client.get_retry(&format!("/api/chains/{tip}/log"), retry)?;
        let fresh: Vec<LogEntry> = log
            .entries
            .iter()
            .filter(|e| e.seq > cursor)
            .cloned()
            .collect();
        cursor = max_seq(&log.entries).max(cursor);

        if !fresh.is_empty() {
            let feedback: Chain = client.get_retry(&format!("/api/chains/{tip}"), retry)?;
            let resp = WaitResult {
                cursor,
                entries: fresh,
                feedback,
            };
            print_wait(&resp, args.oneline)?;
            return Ok(());
        }
        // Nothing new: park on the websocket until the head advances.
        wait_for_entry(&client, &log.entries, retry)?;
    }
}

/// Park the websocket as a doorbell: subscribe the chain's changes at their
/// current heads (no backlog replay) and block until the first live frame, then
/// return so the caller re-drains the log. Rides out restarts.
fn wait_for_entry(client: &Client, entries: &[LogEntry], retry: Retry) -> Result<()> {
    let subs = heads(entries);
    loop {
        let mut socket = client.ws_connect(&subs, retry)?;
        if next_text(&mut socket).is_some() {
            return Ok(()); // an entry landed
        }
        // close/error: reconnect.
    }
}

fn print_wait(resp: &WaitResult, oneline: bool) -> Result<()> {
    if !oneline {
        return print_json(resp);
    }
    println!(
        "cursor={} state={}",
        resp.cursor,
        resp.feedback.state.as_str()
    );
    print_oneline_entries(&resp.entries);
    Ok(())
}
