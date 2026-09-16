//! `nit log` — print the log of the selected changes.
//!
//! Prints entries by global `sequence`. With `--follow`, keeps printing
//! new entries as the server writes them. With `--wait`, blocks until an
//! entry lands past a cursor, prints it, and exits.

use anyhow::{Context, Result, anyhow, bail};

use nit_types::domain::{LogEntry, LogPayload};
use nit_types::events::{StreamMessage, Subscription};

use super::client::{Client, Retry, ServerOpt, next_text, retry_delay, server_url};
use super::format::{print_entries, print_oneline_entries};
use super::format::{render_entries, render_oneline_entries, tagged_digest};
use super::resolve::{SelectArgs, Selection};

#[derive(clap::Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent CLI flags, not encodable state"
)]
pub struct LogArgs {
    /// Which entries to print, by global `sequence`: `3`, `5..9`, `5..`,
    /// `..9`, or `..` for all (the default). A range may include sequences
    /// that belong to other changes; those print nothing. With
    /// `--follow`/`--wait`: one `sequence` cursor, and entries above it
    /// print.
    #[arg(default_value = "..")]
    pub ranges: Vec<String>,
    #[command(flatten)]
    pub select: SelectArgs,
    /// Print the terse one-line-per-entry digest instead of the full rendering.
    #[arg(long)]
    pub oneline: bool,
    /// Print the entries past the cursor, then keep printing each new entry
    /// as the server writes it, until stopped. Survives a server restart.
    #[arg(long)]
    pub follow: bool,
    /// Block until entries exist past the cursor, print the digest and
    /// then those entries, then exit.
    #[arg(long, conflicts_with = "follow")]
    pub wait: bool,
    /// Print the reviews and the lifecycle changes only: drop your own
    /// `revision`, `comment` and `tags` entries. Works with every mode.
    #[arg(long)]
    pub incoming: bool,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Prints the selected changes' log entries by global `sequence`.
///
/// With `--follow` or `--wait`, prints the entries past a cursor and then
/// waits for new ones.
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
        return Follower {
            client: &client,
            selection: &args.select.resolve(&client, Retry::No)?,
            cursor,
            oneline: args.oneline,
            incoming: args.incoming,
            once: args.wait,
            sink: &mut |text: &str| {
                println!("{text}");
                Ok(())
            },
        }
        .run()
        .map(|_| ());
    }
    let ranges = args
        .ranges
        .iter()
        .map(|s| LogRange::parse(s))
        .collect::<Result<Vec<_>>>()?;
    let selection = args.select.resolve(&client, Retry::No)?;
    let entries: Vec<LogEntry> = selection
        .log(&client, None, Retry::No)?
        .into_iter()
        .filter(|e| ranges.iter().any(|r| r.contains(e.sequence)))
        .filter(|e| !(args.incoming && dropped_by_incoming(e)))
        .collect();
    if args.oneline {
        print_oneline_entries(&entries);
    } else {
        print_entries(&entries);
    }
    Ok(())
}

/// Sends every entry the selection gains to `sink`, and never returns.
///
/// This is `--follow` with somewhere other than stdout to write to.
/// `nit watch` follows this way and sends each batch to the session.
///
/// # Errors
///
/// When the server returns a malformed response, a fatal client error, or
/// the sink fails.
pub(super) fn follow(
    client: &Client,
    selection: &Selection,
    incoming: bool,
    sink: &mut dyn FnMut(&str) -> Result<()>,
) -> Result<()> {
    Follower {
        client,
        selection,
        cursor: 0,
        oneline: false,
        incoming,
        once: false,
        sink,
    }
    .run()
    .map(|_| ())
}

/// `--follow` and `--wait`: the selection, the cursor, and how to print.
struct Follower<'a> {
    client: &'a Client,
    selection: &'a Selection,
    /// The highest `sequence` seen. Entries above it are new.
    cursor: u64,
    oneline: bool,
    incoming: bool,
    /// Return after the first batch of new entries. That is `--wait`.
    once: bool,
    /// Where a printed batch goes. `nit log` writes it to stdout, and
    /// `nit watch` posts it to the session's inbox.
    sink: &'a mut dyn FnMut(&str) -> Result<()>,
}

impl Follower<'_> {
    /// A follower rides out a server restart.
    const RETRY: Retry = Retry::UntilUp;

    /// Prints the selection's log entries with `sequence > cursor`, then
    /// keeps printing new ones as they arrive.
    ///
    /// It reads the log past the cursor and prints the new entries. Then
    /// it opens a websocket and prints each entry the server sends. A
    /// `tags` entry can be the first the socket sends for a change,
    /// because the change got the tag with that entry, and its earlier
    /// entries exist only in the log. So a `tags` frame makes it read the
    /// log past the cursor again. A closed socket (a server restart) makes
    /// it start over.
    ///
    /// With `once`, the first batch of new entries prints the digest, then
    /// the entries, and the function returns. With `incoming`, the
    /// author's own entries move the cursor but do not count as new, so
    /// `--wait` returns only on a review or a lifecycle change.
    ///
    /// Returns the cursor that the printed batch ended on. Without
    /// `once` it never returns, because `--follow` runs until it is
    /// stopped.
    ///
    /// # Errors
    ///
    /// When the server returns a malformed response, a fatal client error,
    /// or stdout can't be written.
    fn run(mut self) -> Result<u64> {
        // A server that accepts the socket and then drops it, which an
        // overflowed broadcast does, would otherwise spin the reconnect.
        let mut silent_reconnects = 0;
        loop {
            let entries = self
                .selection
                .log(self.client, Some(self.cursor), Self::RETRY)?;
            if self.print_new(entries)? {
                return Ok(self.cursor);
            }
            // The socket sends only entries past the cursor, and the caller
            // has printed everything up to it.
            let subscription = Subscription {
                query: self.selection.change_query(),
                after: Some(self.cursor),
            };
            let mut socket = self.client.ws_connect(&subscription)?;
            let mut sent_anything = false;
            while let Some(text) = next_text(&mut socket) {
                sent_anything = true;
                let Ok(StreamMessage::Entry(entry)) = serde_json::from_str::<StreamMessage>(&text)
                else {
                    continue;
                };
                let entries = if matches!(entry.payload, LogPayload::Tags(_)) {
                    self.selection
                        .log(self.client, Some(self.cursor), Self::RETRY)?
                } else {
                    vec![entry]
                };
                if self.print_new(entries)? {
                    return Ok(self.cursor);
                }
            }
            if sent_anything {
                silent_reconnects = 0;
            } else {
                std::thread::sleep(retry_delay(silent_reconnects));
                silent_reconnects += 1;
            }
        }
    }

    /// Prints the entries past the cursor and moves the cursor past them.
    ///
    /// Returns `true` when `once` is set and something printed, so `run`
    /// returns.
    fn print_new(&mut self, entries: Vec<LogEntry>) -> Result<bool> {
        let since = self.cursor;
        self.cursor = max_seq(&entries).max(since);
        let fresh: Vec<LogEntry> = entries
            .into_iter()
            .filter(|e| e.sequence > since)
            .filter(|e| !(self.incoming && dropped_by_incoming(e)))
            .collect();
        if fresh.is_empty() {
            return Ok(false);
        }
        let mut text = String::new();
        if self.once {
            let changes = self.selection.changes(self.client, Retry::UntilUp)?;
            text.push_str(&tagged_digest(
                &self.selection.tags,
                &changes,
                Some(self.cursor),
            ));
            text.push_str("--- new since cursor ---\n");
        }
        text.push_str(&render_selected(&fresh, self.oneline));
        (self.sink)(&text)?;
        Ok(self.once)
    }
}

/// Renders the entries, one line each with `oneline`, else in full.
fn render_selected(entries: &[LogEntry], oneline: bool) -> String {
    if oneline {
        render_oneline_entries(entries)
    } else {
        render_entries(entries)
    }
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

/// Whether `--incoming` drops this log entry.
///
/// It drops the entries the author writes about their own work: a
/// `revision`, a `comment`, a set of `tags`. Reviews and every lifecycle
/// change reach the monitor, `merged` included, because the merge ends
/// the author's work on the change.
fn dropped_by_incoming(entry: &LogEntry) -> bool {
    matches!(
        entry.payload,
        LogPayload::Revision(_) | LogPayload::Comment(_) | LogPayload::Tags(_)
    )
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
    use nit_types::domain::ChangeNumber;
    use nit_types::domain::LifecycleAction;
    use nit_types::domain::RevisionNumber;

    use nit_types::testing::sha;

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
    fn incoming_drops_the_authors_own_entries() {
        use nit_types::domain::Verdict;
        use nit_types::domain::{CommentInput, ReviewPayload, RevisionPayload};
        let dropped = |payload| {
            dropped_by_incoming(&LogEntry {
                change_number: ChangeNumber::new(1),
                position: 0,
                sequence: 0,
                created_at: String::new(),
                payload,
            })
        };
        let revision = || {
            LogPayload::Revision(RevisionPayload {
                commit_sha: sha(""),
                parent_sha: sha(""),
                fork_sha: sha(""),
                message: String::new(),
                resets_status: true,
            })
        };
        let comment = || {
            LogPayload::Comment(CommentInput {
                thread_id: None,
                revision: None,
                anchor: None,
                body: String::new(),
                resolved: None,
            })
        };
        let review = || {
            LogPayload::Review(ReviewPayload {
                revision: RevisionNumber::new(0),
                verdict: Verdict::Comment,
                message: String::new(),
                comments: vec![],
            })
        };
        let life = |a| LogPayload::lifecycle(a, None, None);
        assert!(dropped(revision()));
        assert!(dropped(comment()));
        assert!(!dropped(review()));
        assert!(!dropped(life(LifecycleAction::Merged)));
        assert!(!dropped(life(LifecycleAction::Abandoned)));
        assert!(!dropped(life(LifecycleAction::Reopened)));
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
