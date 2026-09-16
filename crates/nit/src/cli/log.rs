//! `nit log` — print the log of the selected changes.
//!
//! Prints entries by global `sequence`. `nit watch` reads the same log
//! and shares [`dropped_by_incoming`], the rule for which entries the
//! author writes about their own work.

use anyhow::{Context, Result, anyhow, bail};

use nit_types::domain::{LogEntry, LogPayload};

use super::client::{Client, Retry, ServerOpt, server_url};
use super::format::{print_entries, print_oneline_entries};
use super::resolve::SelectArgs;

#[derive(clap::Args)]
pub struct LogArgs {
    /// Which entries to print, by global `sequence`: `3`, `5..9`, `5..`,
    /// `..9`, or `..` for all (the default). A range may include sequences
    /// that belong to other changes; those print nothing.
    #[arg(default_value = "..")]
    pub ranges: Vec<String>,
    #[command(flatten)]
    pub select: SelectArgs,
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

/// Prints the selected changes' log entries by global `sequence`.
///
/// # Errors
///
/// When a range is malformed or the server can't be reached.
pub fn log(args: LogArgs) -> Result<()> {
    let client = Client::new(server_url(args.server.server));
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
}
