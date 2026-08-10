//! `nit log` — read the aggregated chain log.
//!
//! Prints entries by global `sequence`, `--follow`s the log as a parked monitor
//! over the websocket change stream, or `--wait`s for the next entries past a
//! cursor and exits.

use anyhow::{Context, Result, anyhow, bail};

use nit_types::chains::Chain;
use nit_types::domain::LifecycleAction;
use nit_types::events::StreamMessage;
use nit_types::log::{ChainLog, LogEntry, LogPayload};

use super::client::{Client, Retry, ServerOpt, next_text, server_url};
use super::format::{print_chain_digest, print_entries, print_oneline_entries, render_entry};
use super::resolve::resolve_chain;

#[derive(clap::Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent CLI flags, not encodable state"
)]
pub struct LogArgs {
    /// Default (one-shot): global `sequence` values or half-open `sequence` ranges into
    /// the aggregated chain log: `3`, `5..9`, `5..`, `..9`, `..` (all, the
    /// default). A range may span seqs absent from this chain (they belong to
    /// other changes); those simply match nothing. With `--follow`/`--wait`: a
    /// single global `sequence` cursor to stream/drain from.
    #[arg(default_value = "..")]
    pub ranges: Vec<String>,
    /// Chain to read, by its tip change id; overrides the cwd lookup.
    #[arg(long)]
    pub chain: Option<u64>,
    /// Print the terse one-line-per-entry digest instead of the full rendering.
    #[arg(long)]
    pub oneline: bool,
    /// Follow the log: replay from the cursor, then stream each new entry as it
    /// lands — a parked monitor. Rides out restarts; runs until stopped.
    #[arg(long)]
    pub follow: bool,
    /// Block until entries land past the sequence cursor, print them once beneath the
    /// chain digest, then exit — the one-shot wait.
    #[arg(long, conflicts_with = "follow")]
    pub wait: bool,
    /// Keep only the reviewer's activity: drop the author's own entries
    /// (`revision`/`comment`) and the automatic `merged` lifecycle. A filter,
    /// so it composes with any mode — one-shot, `--wait`, or `--follow`.
    #[arg(long)]
    pub reviewer_only: bool,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Prints entries of the aggregated chain log by global `sequence`.
///
/// With `--follow`/`--wait`, streams or drains past a cursor instead.
///
/// # Errors
///
/// When a range is malformed or the server can't be reached.
pub fn log(args: LogArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    if args.follow || args.wait {
        let [spec] = args.ranges.as_slice() else {
            bail!("--follow/--wait take a single starting sequence cursor (e.g. `0` or `..`)");
        };
        let cursor = follow_cursor(spec)?;
        let change_id = resolve_chain(&client, args.chain, Retry::No)?;
        return if args.wait {
            wait(&client, change_id, cursor, args.oneline, args.reviewer_only)
        } else {
            follow(&client, change_id, cursor, args.oneline, args.reviewer_only)
        };
    }
    let change_id = resolve_chain(&client, args.chain, Retry::No)?;
    let log: ChainLog = client.get(&format!("/api/chains/{change_id}/log"))?;
    let ranges = args
        .ranges
        .iter()
        .map(|s| LogRange::parse(s))
        .collect::<Result<Vec<_>>>()?;
    let entries: Vec<LogEntry> = log
        .entries
        .into_iter()
        .filter(|e| ranges.iter().any(|r| r.contains(e.sequence)))
        .filter(|e| !(args.reviewer_only && muted_by_reviewer_only(e)))
        .collect();
    if args.oneline {
        print_oneline_entries(&entries);
    } else {
        print_entries(&entries);
    }
    Ok(())
}

/// Blocks until the chain's aggregated log passes the `sequence` cursor.
///
/// Prints the chain digest and the entries past the cursor, then exits. Each
/// pass drains `(cursor, head]` from the log (the source of truth); otherwise
/// it parks the websocket as a doorbell until any new entry lands. Rides out
/// restarts. `reviewer_only` drops the author's own entries, so the wait blocks
/// until reviewer activity lands (its own echoes advance the cursor but don't
/// wake it).
///
/// # Errors
///
/// When the server returns a malformed response or a fatal client error.
fn wait(
    client: &Client,
    change_id: u64,
    mut cursor: u64,
    oneline: bool,
    reviewer_only: bool,
) -> Result<()> {
    let retry = Retry::UntilUp;
    loop {
        let log: ChainLog = client.get_retry(&format!("/api/chains/{change_id}/log"), retry)?;
        let fresh: Vec<LogEntry> = log
            .entries
            .iter()
            .filter(|e| e.sequence > cursor)
            .filter(|e| !(reviewer_only && muted_by_reviewer_only(e)))
            .cloned()
            .collect();
        cursor = max_seq(&log.entries).max(cursor);
        if !fresh.is_empty() {
            let chain: Chain = client.get_retry(&format!("/api/chains/{change_id}"), retry)?;
            print_chain_digest(client, &chain, Some(cursor))?;
            println!("--- new since cursor ---");
            if oneline {
                print_oneline_entries(&fresh);
            } else {
                print_entries(&fresh);
            }
            return Ok(());
        }
        wait_for_entry(client, &log.entries, retry)?;
    }
}

/// Parks the websocket as a doorbell.
///
/// Subscribes the chain's changes at their current heads (no backlog replay)
/// and blocks until the first live frame, then returns so the caller re-drains
/// the log. Rides out restarts.
fn wait_for_entry(client: &Client, entries: &[LogEntry], retry: Retry) -> Result<()> {
    let subs = heads(entries);
    loop {
        let mut socket = client.ws_connect(&subs, retry)?;
        if next_text(&mut socket).is_some() {
            return Ok(());
        }
        // close/error: reconnect.
    }
}

/// Follows the aggregated chain log as a parked monitor.
///
/// Replays `(cursor, head]`, then relays each new entry as it lands, until
/// stopped. Rides out restarts (reconnect re-reads the gap from the log).
/// `reviewer_only` drops the author's own entries (`revision`/`comment`).
///
/// # Errors
///
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
        // Each connect refetches and replays past the cursor, so a reconnect
        // (server restart, overflow) re-reads whatever landed during the gap.
        let log: ChainLog = client.get_retry(&format!("/api/chains/{change_id}/log"), retry)?;
        for e in &log.entries {
            if e.sequence > cursor {
                cursor = cursor.max(e.sequence);
                relay(e, oneline, reviewer_only);
            }
        }
        let mut socket = client.ws_connect(&heads(&log.entries), retry)?;
        // None (close/error) falls through to the outer loop, which reconnects.
        while let Some(text) = next_text(&mut socket) {
            // Cursor mode never asks for a projection, so only `entry` frames
            // arrive; ignore anything else.
            let Ok(StreamMessage::Entry(entry)) = serde_json::from_str::<StreamMessage>(&text)
            else {
                continue;
            };
            cursor = cursor.max(entry.sequence);
            relay(&entry, oneline, reviewer_only);
        }
    }
}

fn relay(entry: &LogEntry, oneline: bool, reviewer_only: bool) {
    if reviewer_only && muted_by_reviewer_only(entry) {
        return;
    }
    if oneline {
        print_oneline_entries(std::slice::from_ref(entry));
    } else {
        println!("{}\n", render_entry(entry));
    }
}

/// Each change's head position (max position + 1) from the aggregated log.
///
/// The from-position to subscribe at so the backlog replay is empty (doorbell
/// mode).
fn heads(entries: &[LogEntry]) -> std::collections::HashMap<u64, u64> {
    let mut heads: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for e in entries {
        heads
            .entry(e.change_id)
            .and_modify(|h| *h = (*h).max(e.position + 1))
            .or_insert(e.position + 1);
    }
    heads
}

fn max_seq(entries: &[LogEntry]) -> u64 {
    entries.iter().map(|e| e.sequence).max().unwrap_or(0)
}

/// Parses the single `--follow` positional into a starting `sequence` cursor.
///
/// A bare `N` follows from `N`, `..` from `0`.
fn follow_cursor(spec: &str) -> Result<u64> {
    let spec = spec.trim();
    if spec == ".." || spec.is_empty() {
        return Ok(0);
    }
    spec.trim_end_matches("..")
        .parse::<u64>()
        .with_context(|| format!("bad sequence cursor {spec:?}"))
}

/// Whether `--reviewer-only` suppresses this log entry.
///
/// It suppresses the author's own echoes (`revision`/`comment`) and the
/// automatic `merged` lifecycle (written by the merge timer, not the
/// reviewer). Reviewer verdicts and the reviewer-driven `abandoned`/`reopened`
/// lifecycle always reach the monitor.
fn muted_by_reviewer_only(entry: &LogEntry) -> bool {
    match &entry.payload {
        LogPayload::Revision(_) | LogPayload::Comment(_) => true,
        LogPayload::Lifecycle(p) => p.action == LifecycleAction::Merged,
        LogPayload::Review(_) => false,
    }
}

/// A parsed `nit log` selector over global `sequence`, half-open.
///
/// `contains(sequence)` tests membership; a bare `N` is the singleton `[N, N+1)`.
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

    fn contains(self, sequence: u64) -> bool {
        match self {
            LogRange::Open { from } => sequence >= from,
            LogRange::Closed { from, to } => sequence >= from && sequence < to,
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
    fn log_range_contains_by_seq() {
        // A bare `N` is the singleton `[N, N+1)`.
        assert!(LogRange::Closed { from: 3, to: 4 }.contains(3));
        assert!(!LogRange::Closed { from: 3, to: 4 }.contains(2));
        assert!(!LogRange::Closed { from: 3, to: 4 }.contains(4));
        // Half-open: end excluded.
        assert!(LogRange::Closed { from: 3, to: 6 }.contains(5));
        assert!(!LogRange::Closed { from: 3, to: 6 }.contains(6));
        // Open-ended matches every sequence at or past `from`.
        assert!(!LogRange::Open { from: 2 }.contains(1));
        assert!(LogRange::Open { from: 2 }.contains(2));
        assert!(LogRange::Open { from: 2 }.contains(1000));
    }

    #[test]
    fn reviewer_only_mutes_agent_echoes_and_auto_merge() {
        use nit_types::domain::Verdict;
        use nit_types::log::{CommentInput, ReviewPayload, RevisionPayload};
        let muted = |payload| {
            muted_by_reviewer_only(&LogEntry {
                change_id: 1,
                position: 0,
                sequence: 0,
                created_at: String::new(),
                payload,
            })
        };
        let revision = || {
            LogPayload::Revision(RevisionPayload {
                commit_sha: String::new(),
                parent_sha: String::new(),
                fork_sha: String::new(),
                message: String::new(),
                resets_status: true,
            })
        };
        let comment = || {
            LogPayload::Comment(CommentInput {
                thread_id: None,
                revision: None,
                file: None,
                line: None,
                side: None,
                range: None,
                line_text: None,
                body: String::new(),
                resolved: None,
            })
        };
        let review = || {
            LogPayload::Review(ReviewPayload {
                revision: 0,
                verdict: Verdict::Comment,
                message: String::new(),
                comments: vec![],
            })
        };
        let life = |a| LogPayload::lifecycle(a, None, None);
        assert!(muted(revision()));
        assert!(muted(comment()));
        // The automatic merge is the timer's, not reviewer activity.
        assert!(muted(life(LifecycleAction::Merged)));
        // Reviewer activity and reviewer-driven lifecycle reach the monitor.
        assert!(!muted(review()));
        assert!(!muted(life(LifecycleAction::Abandoned)));
        assert!(!muted(life(LifecycleAction::Reopened)));
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
