//! `nit log` — print entries of the aggregated chain log by position/range, or
//! `--follow` it as a parked monitor over the websocket change stream.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use super::client::{Client, Retry, ServerOpt, next_text, print_json, server_url};
use super::format::print_oneline_entries;
use super::resolve::resolve_chain;

#[derive(clap::Args)]
pub struct LogArgs {
    /// Without `--follow`: entry positions or half-open ranges into the
    /// aggregated chain log (sorted by global seq): `3`, `5..9`, `5..`, `..9`,
    /// `..` (all, the default). With `--follow`: a single global `seq` cursor
    /// to stream from.
    #[arg(default_value = "..")]
    pub ranges: Vec<String>,
    /// Chain to read, by its tip change id; overrides the cwd lookup.
    #[arg(long)]
    pub chain: Option<u64>,
    /// Print a one-line digest per entry instead of full payloads
    #[arg(long)]
    pub oneline: bool,
    /// Follow the log: replay from the cursor, then stream each new entry as it
    /// lands — a parked monitor. Rides out restarts; runs until stopped.
    #[arg(long)]
    pub follow: bool,
    /// With `--follow`, relay only the reviewer's activity: drop the agent's
    /// own entries (`revision`/`comment`) and the automatic `merged`
    /// lifecycle.
    #[arg(long, requires = "follow")]
    pub reviewer_only: bool,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Print entries of the aggregated chain log by position/range.
///
/// # Errors
/// When a range is malformed or the server can't be reached.
pub fn log(args: LogArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    if args.follow {
        let [spec] = args.ranges.as_slice() else {
            bail!("--follow takes a single starting seq cursor (e.g. `0` or `..`)");
        };
        let cursor = follow_cursor(spec)?;
        let change_id = resolve_chain(&client, args.chain, Retry::No)?;
        return follow(&client, change_id, cursor, args.oneline, args.reviewer_only);
    }
    let change_id = resolve_chain(&client, args.chain, Retry::No)?;
    let log: Value = client.get(&format!("/api/chains/{change_id}/log"))?;
    let all = log["entries"].as_array().cloned().unwrap_or_default();
    let mut entries: Vec<Value> = Vec::new();
    for spec in &args.ranges {
        let (from, to) = LogRange::parse(spec)?.bounds(all.len());
        entries.extend(all.get(from..to).unwrap_or(&[]).iter().cloned());
    }
    if args.oneline {
        print_oneline_entries(&entries);
    } else {
        print_json(&json!({"entries": entries}))?;
    }
    Ok(())
}

/// Follow the aggregated chain log as a parked monitor: replay `(cursor, head]`,
/// then relay each new entry as it lands, until stopped. Rides out restarts
/// (reconnect re-reads the gap from the log). `reviewer_only` drops the agent's
/// own entries (`revision`/`comment`).
///
/// # Errors
/// When a connect fails fatally or stdout can't be written.
fn follow(
    client: &Client,
    change_id: u64,
    mut cursor: u64,
    oneline: bool,
    reviewer_only: bool,
) -> Result<()> {
    let retry = Retry::UntilUp;
    loop {
        // Re-derive the chain each connect: a new tip enters the watch set, a
        // departed change goes quiet (self-healing, never needs new_parent).
        let log: Value = client.get_retry(&format!("/api/chains/{change_id}/log"), retry)?;
        let entries: Vec<Value> = log["entries"].as_array().cloned().unwrap_or_default();
        for e in &entries {
            if e["seq"].as_u64().unwrap_or(0) > cursor {
                cursor = cursor.max(e["seq"].as_u64().unwrap_or(cursor));
                relay(e, oneline, reviewer_only)?;
            }
        }
        let mut socket = client.ws_connect(&heads(&entries), retry)?;
        // None (close/error) falls through to the outer loop, which reconnects.
        while let Some(text) = next_text(&mut socket) {
            let Ok(entry) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if entry.get("new_parent").is_some() {
                break; // re-derive the chain (picks up the new parent)
            }
            cursor = cursor.max(entry["seq"].as_u64().unwrap_or(cursor));
            relay(&entry, oneline, reviewer_only)?;
        }
    }
}

/// Relay one streamed entry, honoring `--reviewer-only`.
fn relay(entry: &Value, oneline: bool, reviewer_only: bool) -> Result<()> {
    if reviewer_only && muted_by_reviewer_only(entry) {
        return Ok(());
    }
    if oneline {
        print_oneline_entries(std::slice::from_ref(entry));
        Ok(())
    } else {
        print_json(entry)
    }
}

/// Each change's head idx (max idx + 1) from the aggregated log — the
/// from-idx to subscribe at so the backlog replay is empty (doorbell mode).
pub(super) fn heads(entries: &[Value]) -> std::collections::HashMap<u64, u64> {
    let mut heads: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for e in entries {
        if let (Some(cid), Some(idx)) = (e["change_id"].as_u64(), e["idx"].as_u64()) {
            heads
                .entry(cid)
                .and_modify(|h| *h = (*h).max(idx + 1))
                .or_insert(idx + 1);
        }
    }
    heads
}

pub(super) fn max_seq(entries: &[Value]) -> u64 {
    entries
        .iter()
        .filter_map(|e| e["seq"].as_u64())
        .max()
        .unwrap_or(0)
}

/// Parse the single `--follow` positional into a starting `seq` cursor: a bare
/// `N` follows from `N`, `..` from `0`.
fn follow_cursor(spec: &str) -> Result<u64> {
    let spec = spec.trim();
    if spec == ".." || spec.is_empty() {
        return Ok(0);
    }
    spec.trim_end_matches("..")
        .parse::<u64>()
        .with_context(|| format!("bad seq cursor {spec:?}"))
}

/// A log entry `--reviewer-only` suppresses: the agent's own echoes
/// (`revision`/`comment`) and the automatic `merged` lifecycle (written by the
/// merge timer, not the reviewer). Reviewer verdicts and the reviewer-driven
/// `abandoned`/`reopened` lifecycle always reach the monitor. Unrecognized
/// kinds fail open.
fn muted_by_reviewer_only(entry: &Value) -> bool {
    match entry["kind"].as_str() {
        Some("revision" | "comment") => true,
        Some("lifecycle") => entry["payload"]["action"].as_str() == Some("merged"),
        _ => false,
    }
}

/// A parsed `nit log` position selector, half-open. `bounds(len)` clamps to the
/// aggregated log's length (positions, not idx/seq).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogRange {
    Open { from: u64 },
    Closed { from: u64, to: u64 },
}

impl LogRange {
    fn parse(spec: &str) -> Result<LogRange> {
        let num = |s: &str| -> Result<u64> {
            s.trim()
                .parse::<u64>()
                .with_context(|| format!("bad index {:?}", s.trim()))
        };
        let Some((a, b)) = spec.split_once("..") else {
            let from = num(spec)?;
            let to = from
                .checked_add(1)
                .ok_or_else(|| anyhow!("index {from} too large"))?;
            return Ok(LogRange::Closed { from, to });
        };
        let from = if a.trim().is_empty() { 0 } else { num(a)? };
        if b.trim().is_empty() {
            return Ok(LogRange::Open { from });
        }
        let to = num(b)?;
        if to <= from {
            bail!("empty or reversed range {spec:?}: the end must be greater than the start");
        }
        Ok(LogRange::Closed { from, to })
    }

    fn bounds(self, len: usize) -> (usize, usize) {
        let clamp = |n: u64| usize::try_from(n).unwrap_or(usize::MAX).min(len);
        match self {
            LogRange::Open { from } => (clamp(from), len),
            LogRange::Closed { from, to } => {
                let from = clamp(from);
                (from, clamp(to).max(from))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_range_forms_and_rejections() {
        let ok = |s: &str| LogRange::parse(s).expect("range should parse");
        assert_eq!(ok("3"), LogRange::Closed { from: 3, to: 4 });
        assert_eq!(ok("3..6"), LogRange::Closed { from: 3, to: 6 });
        assert_eq!(ok("3.."), LogRange::Open { from: 3 });
        assert_eq!(ok("..6"), LogRange::Closed { from: 0, to: 6 });
        assert_eq!(ok(".."), LogRange::Open { from: 0 });
        assert!(LogRange::parse("6..6").is_err());
        assert!(LogRange::parse("6..3").is_err());
        assert!(LogRange::parse("-1").is_err());
        assert!(LogRange::parse("notanumber").is_err());
    }

    #[test]
    fn log_range_bounds_clamp_to_len() {
        assert_eq!(LogRange::Open { from: 2 }.bounds(5), (2, 5));
        assert_eq!(LogRange::Open { from: 9 }.bounds(5), (5, 5));
        assert_eq!(LogRange::Closed { from: 1, to: 3 }.bounds(5), (1, 3));
        assert_eq!(LogRange::Closed { from: 1, to: 9 }.bounds(5), (1, 5));
        assert_eq!(LogRange::Closed { from: 9, to: 10 }.bounds(5), (5, 5));
    }

    #[test]
    fn reviewer_only_mutes_agent_echoes_and_auto_merge() {
        let kind = |k: &str| muted_by_reviewer_only(&json!({"kind": k, "payload": {}}));
        let life = |a: &str| {
            muted_by_reviewer_only(&json!({"kind": "lifecycle", "payload": {"action": a}}))
        };
        // The agent's own writes.
        assert!(kind("revision"));
        assert!(kind("comment"));
        // The automatic merge is the timer's, not reviewer activity.
        assert!(life("merged"));
        // Reviewer activity and reviewer-driven lifecycle reach the monitor.
        assert!(!kind("review"));
        assert!(!life("abandoned"));
        assert!(!life("reopened"));
        // Unrecognized kinds fail open — relayed, never hidden.
        assert!(!kind("some_future_kind"));
    }

    #[test]
    fn follow_cursor_forms() {
        assert_eq!(follow_cursor("0").expect("zero"), 0);
        assert_eq!(follow_cursor("5").expect("five"), 5);
        assert_eq!(follow_cursor("5..").expect("open"), 5);
        assert_eq!(follow_cursor("..").expect("all"), 0);
        assert!(follow_cursor("nope").is_err());
    }
}
