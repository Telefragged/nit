//! Display helpers shared by 2+ commands — one-line digests of log entries and
//! chains — plus the `--change` / `--change-id` selector flattened into every
//! change-scoped Args struct.

use std::collections::HashMap;

use anyhow::{Result, bail};

use nit_types::chains::Chain;
use nit_types::changes::ChangeDetail;
use nit_types::log::{LogEntry, LogPayload};

use crate::gitscan::short_sha;

use super::client::Client;
use super::resolve::resolve_change;

/// The shared `--change` / `--change-id` selector for change-scoped commands.
#[derive(clap::Args)]
pub struct ChangeTarget {
    /// The change, by its numeric id.
    #[arg(
        long,
        conflicts_with = "change_id",
        required_unless_present = "change_id"
    )]
    pub change: Option<u64>,
    /// The change, by its `Change-Id:` trailer.
    #[arg(long)]
    pub change_id: Option<String>,
}

impl ChangeTarget {
    /// Resolve to a numeric change id, querying the server for a `Change-Id:`.
    pub(crate) fn resolve(&self, client: &Client) -> Result<u64> {
        match (self.change, self.change_id.as_deref()) {
            (Some(id), _) => Ok(id),
            (None, Some(key)) => resolve_change(client, key),
            (None, None) => bail!("pass --change <id> or --change-id <Change-Id>"),
        }
    }
}

pub(crate) fn print_oneline_entries(entries: &[LogEntry]) {
    for e in entries {
        println!(
            "{}\t{}\t{}",
            e.idx,
            e.payload.kind().as_str(),
            entry_summary(e)
        );
    }
}

/// One-line digest of a log entry (a CLI display concern; the server ships only
/// the raw entry).
fn entry_summary(entry: &LogEntry) -> String {
    let change = entry.change_id;
    match &entry.payload {
        LogPayload::Revision(p) => {
            format!("change {change} new revision {}", short_sha(&p.commit_sha))
        }
        LogPayload::Review(p) => format!(
            "reviewer {} on change {change} r{} ({} comment(s))",
            p.verdict.as_str(),
            p.revision,
            p.comments.len()
        ),
        LogPayload::Comment(c) => match c.thread_id {
            Some(thread) => format!("agent commented on thread {thread} (change {change})"),
            None => format!("agent opened a thread on change {change}"),
        },
        LogPayload::Lifecycle(p) => format!("change {change} {}", p.action.as_str()),
    }
}

/// Fetch each member's open-thread count, then print the chain digest: a
/// `state=` header (prefixed with `cursor=` when following) and one aligned line
/// per member — `position change_key status rN Nu subject`. The chain path
/// carries only structure, so the counts come from each member's change snapshot
/// (`GET /api/changes/{id}`); the fold is in memory, so each is a cheap read.
///
/// # Errors
/// When a member's change snapshot can't be fetched.
pub(crate) fn print_chain_digest(
    client: &Client,
    chain: &Chain,
    cursor: Option<u64>,
) -> Result<()> {
    let unresolved = member_unresolved(client, chain)?;
    print!("{}", chain_digest(chain, &unresolved, cursor));
    Ok(())
}

/// Unresolved-thread count per member, scoped to the revision the path pins.
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

fn chain_digest(chain: &Chain, unresolved: &HashMap<u64, u64>, cursor: Option<u64>) -> String {
    use std::fmt::Write;
    let inf = "write to String is infallible";
    let mut out = String::new();
    match cursor {
        Some(seq) => writeln!(out, "cursor={seq} state={}", chain.state.as_str()),
        None => writeln!(out, "state={}", chain.state.as_str()),
    }
    .expect(inf);
    let rows: Vec<[String; 5]> = chain
        .path
        .iter()
        .map(|m| {
            [
                m.position.to_string(),
                short_key(&m.change_key),
                m.status.as_str().to_string(),
                format!("r{}", m.revision),
                format!("{}u", unresolved.get(&m.change_id).copied().unwrap_or(0)),
            ]
        })
        .collect();
    let widths = column_widths(&rows);
    for (m, cols) in chain.path.iter().zip(&rows) {
        writeln!(out, "{}", aligned_row(cols, widths, &m.subject)).expect(inf);
    }
    out
}

/// The max display width of each column across `rows`, for space alignment.
fn column_widths<const N: usize>(rows: &[[String; N]]) -> [usize; N] {
    let mut widths = [0usize; N];
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    widths
}

/// One aligned row: each fixed cell padded to its column width and two-space
/// separated, then the free-form `tail` field. Shared by the chain digest and
/// the repo list.
pub(crate) fn aligned_row<const N: usize>(
    cells: &[String; N],
    widths: [usize; N],
    tail: &str,
) -> String {
    let body = cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell:<width$}"))
        .collect::<Vec<_>>()
        .join("  ");
    format!("{body}  {tail}")
}

fn short_key(key: &str) -> String {
    key.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_summary_digests_each_kind() {
        use nit_types::enums::{LifecycleAction, Verdict};
        use nit_types::log::{CommentInput, ReviewPayload, RevisionPayload};
        let entry = |payload| LogEntry {
            change_id: 7,
            idx: 0,
            seq: 0,
            created_at: String::new(),
            payload,
        };
        let comment = || CommentInput {
            thread_id: None,
            revision: None,
            file: None,
            line: None,
            side: None,
            range: None,
            line_text: None,
            body: String::new(),
            resolved: None,
        };
        let rev = entry(LogPayload::Revision(RevisionPayload {
            commit_sha: "abcdef0123456789".to_string(),
            parent_sha: String::new(),
            base_sha: String::new(),
            message: String::new(),
            resets_status: true,
        }));
        assert_eq!(entry_summary(&rev), "change 7 new revision abcdef012345");
        let review = entry(LogPayload::Review(ReviewPayload {
            review_id: 0,
            revision: 2,
            verdict: Verdict::RequestChanges,
            message: String::new(),
            comments: vec![comment(), comment()],
        }));
        assert_eq!(
            entry_summary(&review),
            "reviewer request_changes on change 7 r2 (2 comment(s))"
        );
        let opened = entry(LogPayload::Comment(comment()));
        assert_eq!(entry_summary(&opened), "agent opened a thread on change 7");
        let life = entry(LogPayload::lifecycle(LifecycleAction::Merged, None, None));
        assert_eq!(entry_summary(&life), "change 7 merged");
    }

    #[test]
    fn chain_digest_aligns_columns_and_headers() {
        use nit_types::chains::PathEntry;
        use nit_types::enums::{ChainState, ChangeStatus};
        let member = |change_id, position, key: &str, status, revision, subject: &str| PathEntry {
            change_id,
            position,
            change_key: key.to_string(),
            status,
            revision,
            subject: subject.to_string(),
            commit_sha: String::new(),
        };
        let chain = Chain {
            tip_change_id: 2,
            repo_id: 1,
            state: ChainState::AgentsTurn,
            path: vec![
                member(
                    1,
                    0,
                    "I0123456789abc",
                    ChangeStatus::ChangesRequested,
                    2,
                    "server: add health endpoint",
                ),
                member(
                    2,
                    1,
                    "Iabcdef0123456",
                    ChangeStatus::Approved,
                    1,
                    "web: render the diff",
                ),
            ],
        };
        let unresolved = HashMap::from([(1, 3), (2, 0)]);
        // Columns padded to the widest cell (here `changes_requested`), no tabs.
        assert_eq!(
            chain_digest(&chain, &unresolved, None),
            "state=agents_turn\n\
             0  I0123456  changes_requested  r2  3u  server: add health endpoint\n\
             1  Iabcdef0  approved           r1  0u  web: render the diff\n"
        );
        // The `--wait` form prefixes the header with the cursor.
        assert!(
            chain_digest(&chain, &unresolved, Some(14))
                .starts_with("cursor=14 state=agents_turn\n")
        );
    }
}
