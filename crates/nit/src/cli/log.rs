//! `nit log`: print the log of a change.
//!
//! Prints entries by `position`. `nit watch` shares
//! [`dropped_by_incoming`], the rule for which entries the author writes
//! about their own work.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow, bail};

use nit_types::domain::{ChangeNumber, LogEntry, LogPayload};
use nit_types::log::Log;

use super::client::{Client, Retry, ServerOpt, server_url};
use super::format::{print_entries, print_oneline_entries};
use super::snippet::Sources;

#[derive(clap::Args)]
pub struct LogArgs {
    /// The change number of the change to print.
    pub change: ChangeNumber,
    /// Which entries to print, by `position`: `3`, `5..9`, `5..`, `..9`, or
    /// `..` for all (the default).
    #[arg(default_value = "..")]
    pub ranges: Vec<String>,
    /// Print the terse one-line-per-entry digest instead of the full rendering.
    #[arg(long)]
    pub oneline: bool,
    /// Print the reviews and the lifecycle changes only: drop your own
    /// `revision`, `comment` and `tags` entries.
    #[arg(long)]
    pub incoming: bool,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Prints the log entries of a change by `position`.
///
/// # Errors
///
/// When a range is malformed, the change is unknown, or the CLI cannot
/// reach the server.
pub fn log(args: LogArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
    let ranges = args
        .ranges
        .iter()
        .map(|s| LogRange::parse(s))
        .collect::<Result<Vec<_>>>()?;
    // Overlapping ranges read an entry twice, and the map keeps it once.
    let mut by_position = BTreeMap::new();
    for range in ranges {
        let log: Log = client.get(&format!(
            "/api/changes/{}/log?{}",
            args.change,
            range.query()
        ))?;
        by_position.extend(log.entries.into_iter().map(|e| (e.position, e)));
    }
    let entries: Vec<LogEntry> = by_position
        .into_values()
        .filter(|e| !(args.incoming && dropped_by_incoming(e)))
        .collect();
    if args.oneline {
        print_oneline_entries(&entries);
    } else {
        print_entries(&entries, &Sources::fetch(&client, &entries, Retry::No)?);
    }
    Ok(())
}

/// Whether `--incoming` drops this log entry.
///
/// It drops the entries the author writes about their own work: a
/// `revision`, a `comment`, a set of `tags`. Reviews and every lifecycle
/// change reach the reader, `merged` included, because the merge ends
/// the author's work on the change.
pub(super) fn dropped_by_incoming(entry: &LogEntry) -> bool {
    matches!(
        entry.payload,
        LogPayload::Revision(_) | LogPayload::Comment(_) | LogPayload::Tags(_)
    )
}

/// A parsed `nit log` selector over `position`, half-open.
///
/// A bare `N` is the singleton `[N, N+1)`.
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

    /// The query string of `GET /api/changes/{id}/log` that reads the range.
    fn query(self) -> String {
        match self {
            LogRange::Open { from } => format!("from={from}"),
            LogRange::Closed { from, to } => format!("from={from}&to={to}"),
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
    fn log_range_queries_its_bounds() {
        assert_eq!(LogRange::Closed { from: 3, to: 4 }.query(), "from=3&to=4");
        assert_eq!(LogRange::Open { from: 2 }.query(), "from=2");
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
}
