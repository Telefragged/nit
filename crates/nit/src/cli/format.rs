//! Display helpers shared by 2+ commands — one-line digests of log entries and
//! chains — plus the `--change` / `--change-id` selector flattened into every
//! change-scoped Args struct.

use std::collections::HashMap;

use anyhow::{Result, bail};
use serde_json::Value;

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

pub(crate) fn print_oneline_entries(entries: &[Value]) {
    for e in entries {
        let idx = e["idx"]
            .as_u64()
            .map_or_else(|| "?".to_string(), |i| i.to_string());
        let kind = e["kind"].as_str().unwrap_or("?");
        println!("{idx}\t{kind}\t{}", entry_summary(e));
    }
}

/// One-line digest of a log entry (a CLI display concern; the server ships only
/// the raw entry).
fn entry_summary(entry: &Value) -> String {
    let p = &entry["payload"];
    let change = entry["change_id"].as_u64().unwrap_or(0);
    match entry["kind"].as_str().unwrap_or("?") {
        "revision" => format!(
            "change {change} new revision {}",
            short_sha(p["commit_sha"].as_str().unwrap_or(""))
        ),
        "review" => format!(
            "reviewer {} on change {change} r{} ({} comment(s))",
            p["verdict"].as_str().unwrap_or("?"),
            p["revision"].as_u64().unwrap_or(0),
            p["comments"].as_array().map_or(0, Vec::len)
        ),
        "comment" => match p["thread_id"].as_u64() {
            Some(thread) => format!("agent commented on thread {thread} (change {change})"),
            None => format!("agent opened a thread on change {change}"),
        },
        "lifecycle" => format!("change {change} {}", p["action"].as_str().unwrap_or("?")),
        other => format!("{other} entry"),
    }
}

/// Compact one-line-per-change digest of a `Chain` for `nit status --oneline`.
/// `unresolved` maps a member's `change_id` to its open-thread count, composed
/// by the caller from the change snapshots (the path itself carries only
/// structure).
pub(crate) fn chain_oneline(chain: &Value, unresolved: &HashMap<u64, u64>) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let inf = "write to String is infallible";
    writeln!(out, "state={}", chain["state"].as_str().unwrap_or("?")).expect(inf);
    let path = chain["path"].as_array().map_or(&[][..], Vec::as_slice);
    for m in path {
        let change_id = m["change_id"].as_u64().unwrap_or(0);
        writeln!(
            out,
            "{}\t{}\t{}\tr{}\t{}u\t{}",
            m["position"].as_u64().unwrap_or(0),
            short_key(m["change_key"].as_str().unwrap_or("")),
            m["status"].as_str().unwrap_or("?"),
            m["revision"].as_u64().unwrap_or(0),
            unresolved.get(&change_id).copied().unwrap_or(0),
            m["subject"].as_str().unwrap_or(""),
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
    use serde_json::json;

    #[test]
    fn entry_summary_digests_each_kind() {
        let rev = json!({"change_id": 7, "kind": "revision", "payload": {"commit_sha": "abcdef0123456789"}});
        assert_eq!(entry_summary(&rev), "change 7 new revision abcdef012345");
        let review = json!({"change_id": 7, "kind": "review",
            "payload": {"verdict": "request_changes", "revision": 2, "comments": [{}, {}]}});
        assert_eq!(
            entry_summary(&review),
            "reviewer request_changes on change 7 r2 (2 comment(s))"
        );
        let opened = json!({"change_id": 7, "kind": "comment", "payload": {"thread_id": null}});
        assert_eq!(entry_summary(&opened), "agent opened a thread on change 7");
        let life = json!({"change_id": 7, "kind": "lifecycle", "payload": {"action": "merged"}});
        assert_eq!(entry_summary(&life), "change 7 merged");
    }

    #[test]
    fn chain_oneline_digests_each_member() {
        // The path carries only structure; the unresolved counts are composed
        // separately (from the change snapshots) and keyed by change_id.
        let chain = json!({
            "state": "agents_turn",
            "path": [
                {"change_id": 1, "position": 0, "change_key": "I0123456789abc",
                 "status": "changes_requested", "revision": 2,
                 "subject": "server: add health endpoint"},
                {"change_id": 2, "position": 1, "change_key": "Iabcdef0123456",
                 "status": "approved", "revision": 1, "subject": "web: render the diff"},
            ]
        });
        let unresolved = HashMap::from([(1, 3), (2, 0)]);
        assert_eq!(
            chain_oneline(&chain, &unresolved),
            "state=agents_turn\n\
             0\tI01234567\tchanges_requested\tr2\t3u\tserver: add health endpoint\n\
             1\tIabcdef01\tapproved\tr1\t0u\tweb: render the diff\n"
        );
    }
}
