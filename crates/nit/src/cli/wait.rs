//! `nit wait` — block until the chain's aggregated log holds something worth
//! acting on past the `seq` cursor, parking the websocket as a doorbell.

use anyhow::Result;
use serde_json::{Value, json};

use super::client::{Client, Retry, print_json, server_url};
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
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
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
    let client = Client::new(server_url(args.server));
    let retry = Retry::UntilUp;
    let mut cursor = args.cursor;
    // HEAD is fixed for this command's lifetime, so the tip change id (the
    // chain's stable identity) resolves once, not every loop pass.
    let tip = resolve_tip_change(&client, retry)?;
    loop {
        let log = client.get_retry(&format!("/api/chains/{tip}/log"), retry)?;
        let entries: Vec<Value> = log["entries"].as_array().cloned().unwrap_or_default();
        let fresh: Vec<Value> = entries
            .iter()
            .filter(|e| e["seq"].as_u64().unwrap_or(0) > cursor)
            .cloned()
            .collect();
        cursor = max_seq(&entries).max(cursor);

        if !fresh.is_empty() {
            let feedback = client.get_retry(&format!("/api/chains/{tip}"), retry)?;
            let resp = json!({"cursor": cursor, "entries": fresh, "feedback": feedback});
            print_wait(&resp, args.oneline)?;
            return Ok(());
        }
        // Nothing new: park on the websocket until the head advances.
        wait_for_entry(&client, &entries, retry)?;
    }
}

/// Park the websocket as a doorbell: subscribe the chain's changes at their
/// current heads (no backlog replay) and block until the first live frame, then
/// return so the caller re-drains the log. Rides out restarts.
fn wait_for_entry(client: &Client, entries: &[Value], retry: Retry) -> Result<()> {
    let subs = heads(entries);
    loop {
        let mut socket = client.ws_connect(&subs, retry)?;
        loop {
            match socket.read() {
                Ok(tungstenite::Message::Text(_)) => return Ok(()), // an entry landed
                Ok(tungstenite::Message::Ping(p)) => {
                    let _ = socket.send(tungstenite::Message::Pong(p));
                }
                Ok(tungstenite::Message::Close(_)) | Err(_) => break, // reconnect
                Ok(_) => {}
            }
        }
    }
}

fn print_wait(resp: &Value, oneline: bool) -> Result<()> {
    if !oneline {
        return print_json(resp);
    }
    let cursor = resp["cursor"].as_u64().unwrap_or(0);
    let state = resp["feedback"]["state"].as_str().unwrap_or("?");
    println!("cursor={cursor} state={state}");
    if let Some(arr) = resp["entries"].as_array() {
        print_oneline_entries(arr);
    }
    Ok(())
}
