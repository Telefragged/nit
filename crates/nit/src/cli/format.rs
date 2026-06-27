//! Display helpers shared by 2+ commands — one-line digests of log entries and
//! chains — plus the `--change` / `--change-id` selector flattened into every
//! change-scoped Args struct.

use std::collections::HashMap;

use anyhow::{Result, bail};

use crate::api::types::{Chain, LogEntry, LogPayload};
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

/// Compact one-line-per-change digest of a `Chain` for `nit status --oneline`.
/// `unresolved` maps a member's `change_id` to its open-thread count, composed
/// by the caller from the change snapshots (the path itself carries only
/// structure).
pub(crate) fn chain_oneline(chain: &Chain, unresolved: &HashMap<u64, u64>) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let inf = "write to String is infallible";
    writeln!(out, "state={}", chain.state.as_str()).expect(inf);
    for m in &chain.path {
        writeln!(
            out,
            "{}\t{}\t{}\tr{}\t{}u\t{}",
            m.position,
            short_key(&m.change_key),
            m.status.as_str(),
            m.revision,
            unresolved.get(&m.change_id).copied().unwrap_or(0),
            m.subject,
        )
        .expect(inf);
    }
    out
}

fn short_key(key: &str) -> String {
    key.chars().take(9).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_summary_digests_each_kind() {
        use crate::api::types::{CommentInput, ReviewPayload, RevisionPayload};
        use crate::enums::{LifecycleAction, Verdict};
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
    fn chain_oneline_digests_each_member() {
        use crate::api::types::PathEntry;
        use crate::enums::{ChainState, ChangeStatus};
        // The path carries only structure; the unresolved counts are composed
        // separately (from the change snapshots) and keyed by change_id.
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
        assert_eq!(
            chain_oneline(&chain, &unresolved),
            "state=agents_turn\n\
             0\tI01234567\tchanges_requested\tr2\t3u\tserver: add health endpoint\n\
             1\tIabcdef01\tapproved\tr1\t0u\tweb: render the diff\n"
        );
    }
}
