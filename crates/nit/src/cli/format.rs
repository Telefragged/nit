//! Display helpers shared by 2+ commands — one-line digests of log entries and
//! chains — plus the `--change` / `--change-id` selector flattened into every
//! change-scoped Args struct.

use std::collections::HashMap;

use anyhow::{Result, bail};

use nit_types::chains::Chain;
use nit_types::changes::ChangeDetail;
use nit_types::comments::CommentRange;
use nit_types::log::{CommentInput, LogEntry, LogPayload};

use crate::gitscan::identity::subject_of;
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

/// The opt-in terse form (`--oneline`): one whitespace-separated line per entry
/// keyed by its global `seq`.
pub(crate) fn print_oneline_entries(entries: &[LogEntry]) {
    for e in entries {
        println!(
            "seq {}  {}  {}",
            e.seq,
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

/// The multi-line rendering of one log entry (no trailing blank line), a pure
/// function of that entry. Two facts a raw entry omits are deliberately not
/// reconstructed, to keep rendering stateless: a `revision` entry shows no minted
/// revision number (it returns once the number rides on the log entry itself),
/// and a reply names only its thread — a reply's anchor lives on the thread's
/// opening entry, not here.
pub(crate) fn render_entry(entry: &LogEntry) -> String {
    let seq = entry.seq;
    let change = entry.change_id;
    match &entry.payload {
        LogPayload::Revision(p) => format!(
            "seq {seq}  change {change}  revision {}  {}",
            short_sha(&p.commit_sha),
            subject_of(&p.message),
        ),
        LogPayload::Review(p) => {
            let mut out = format!(
                "seq {seq}  change {change} r{}  reviewer: {}",
                p.revision,
                p.verdict.as_str()
            );
            if !p.message.is_empty() {
                out.push('\n');
                out.push_str(&indent(&p.message, 4));
            }
            for c in &p.comments {
                out.push('\n');
                out.push_str(&render_comment(c));
            }
            out
        }
        LogPayload::Comment(c) => {
            let head = match comment_target(c) {
                Some(target) => format!("seq {seq}  change {change}  comment on {target}"),
                None => format!("seq {seq}  change {change}  comment"),
            };
            format!("{head}\n{}", indent(&c.body, 4))
        }
        LogPayload::Lifecycle(p) => match &p.message {
            Some(m) if !m.is_empty() => {
                format!("seq {seq}  change {change}  {}: {m}", p.action.as_str())
            }
            _ => format!("seq {seq}  change {change}  {}", p.action.as_str()),
        },
    }
}

/// One comment inside a `review`: `t<id>  <anchor>  [resolved]`, then the body on
/// its own line indented one level deeper. The anchor shows only when this entry
/// opened the thread; a reply carries none.
fn render_comment(c: &CommentInput) -> String {
    let resolved = if c.resolved == Some(true) {
        "  [resolved]"
    } else {
        ""
    };
    let loc = opening_anchor(c);
    let head = match c.thread_id {
        Some(id) => format!("    t{id}{loc}{resolved}"),
        None => format!("    {loc}{resolved}"),
    };
    format!("{head}\n{}", indent(&c.body, 8))
}

/// The `thread N (location)` target of an agent `comment` entry — the location
/// only when this entry opened the thread.
fn comment_target(c: &CommentInput) -> Option<String> {
    let tid = c.thread_id?;
    Some(if c.side.is_some() {
        format!("thread {tid} ({})", anchor(c))
    } else {
        format!("thread {tid}")
    })
}

/// The `  <anchor>` an opening comment carries in its own payload; empty for a
/// reply, whose `side` is unset (its anchor lives on the opening entry).
fn opening_anchor(c: &CommentInput) -> String {
    if c.side.is_some() {
        format!("  {}", anchor(c))
    } else {
        String::new()
    }
}

/// The anchor label of an opening comment.
fn anchor(c: &CommentInput) -> String {
    anchor_str(c.file.as_deref(), c.line, c.range.as_ref())
}

/// The anchor label from raw parts: `(change-level)`, `file:line`, or the full
/// `file:start_line:start_char-end_line:end_char` for a range. Shared by the log
/// renderer and the `nit comment` confirmation.
fn anchor_str(file: Option<&str>, line: Option<u64>, range: Option<&CommentRange>) -> String {
    let Some(file) = file else {
        return "(change-level)".to_string();
    };
    if let Some(r) = range {
        format!(
            "{file}:{}:{}-{}:{}",
            r.start_line, r.start_char, r.end_line, r.end_char
        )
    } else if let Some(line) = line {
        format!("{file}:{line}")
    } else {
        file.to_string()
    }
}

/// Prefix every line of `text` with `n` spaces.
fn indent(text: &str, n: usize) -> String {
    let pad = " ".repeat(n);
    text.lines()
        .map(|line| format!("{pad}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Print each entry's rich rendering, a blank line between entries.
pub(crate) fn print_entries(entries: &[LogEntry]) {
    let blocks: Vec<String> = entries.iter().map(render_entry).collect();
    if !blocks.is_empty() {
        println!("{}", blocks.join("\n\n"));
    }
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
    fn log_render_review_and_revision() {
        use nit_types::comments::CommentRange;
        use nit_types::enums::{Side, Verdict};
        use nit_types::log::{CommentInput, ReviewPayload, RevisionPayload};
        let entry = |change_id, idx, seq, payload| LogEntry {
            change_id,
            idx,
            seq,
            created_at: String::new(),
            payload,
        };
        let opening = |tid, file: Option<&str>, line, range, resolved, body: &str| CommentInput {
            thread_id: Some(tid),
            revision: Some(2),
            file: file.map(String::from),
            line,
            side: Some(Side::New),
            range,
            line_text: None,
            body: body.to_string(),
            resolved,
        };
        let review = entry(
            42,
            5,
            12,
            LogPayload::Review(ReviewPayload {
                review_id: 1,
                revision: 2,
                verdict: Verdict::RequestChanges,
                message: "Cover one.\nCover two.".to_string(),
                comments: vec![
                    opening(3, None, None, None, None, "Change-level question?"),
                    opening(
                        4,
                        Some("src/queue.rs"),
                        Some(42),
                        None,
                        None,
                        "Bounded channel.",
                    ),
                    opening(
                        5,
                        Some("src/queue.rs"),
                        Some(42),
                        Some(CommentRange {
                            start_line: 42,
                            start_char: 8,
                            end_line: 42,
                            end_char: 30,
                        }),
                        Some(true),
                        "Overflow on 32-bit.",
                    ),
                ],
            }),
        );
        // Verdict header, indented cover message, then one comment per line led
        // by its thread id, body indented one level deeper; the range anchor is
        // the full form and the resolved marker sits on the anchor line.
        assert_eq!(
            render_entry(&review),
            "seq 12  change 42 r2  reviewer: request_changes\n\
             \x20   Cover one.\n\
             \x20   Cover two.\n\
             \x20   t3  (change-level)\n\
             \x20       Change-level question?\n\
             \x20   t4  src/queue.rs:42\n\
             \x20       Bounded channel.\n\
             \x20   t5  src/queue.rs:42:8-42:30  [resolved]\n\
             \x20       Overflow on 32-bit."
        );

        // A revision entry shows its short sha and subject — no minted number.
        let rev = |idx, seq, sha: &str, msg: &str| {
            entry(
                42,
                idx,
                seq,
                LogPayload::Revision(RevisionPayload {
                    commit_sha: sha.to_string(),
                    parent_sha: String::new(),
                    base_sha: String::new(),
                    message: msg.to_string(),
                    resets_status: true,
                }),
            )
        };
        assert_eq!(
            render_entry(&rev(0, 3, "abcdef0123456789", "queue: first\n\nbody")),
            "seq 3  change 42  revision abcdef012345  queue: first"
        );
        assert_eq!(
            render_entry(&rev(6, 20, "1234567890abcdef", "queue: second")),
            "seq 20  change 42  revision 1234567890ab  queue: second"
        );
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
