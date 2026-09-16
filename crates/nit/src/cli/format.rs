//! Display helpers shared by 2+ commands.
//!
//! One-line digests of log entries and tag selections, plus the
//! `--change` / `--change-id` selector flattened into every change-scoped
//! Args struct.

use anyhow::{Result, bail};

use nit_types::domain::Anchor;
use nit_types::domain::ChangeId;
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::LineAnchor;
use nit_types::domain::Tags;
use nit_types::domain::ThreadProjection;
use nit_types::domain::{CommentInput, LogEntry, LogPayload};

use crate::gitscan::short_sha;
use nit_types::domain::subject_of;

use super::client::Client;
use super::resolve::resolve_change;

/// The shared `--change` / `--change-id` selector for change-scoped commands.
#[derive(clap::Args)]
pub struct ChangeTarget {
    /// The change, by its number.
    #[arg(
        long,
        conflicts_with = "change_id",
        required_unless_present = "change_id"
    )]
    pub change: Option<ChangeNumber>,
    /// The change, by its `Change-Id:` trailer.
    #[arg(long)]
    pub change_id: Option<String>,
}

impl ChangeTarget {
    /// Resolves to a change number, querying the server for a `Change-Id:`.
    pub(crate) fn resolve(&self, client: &Client) -> Result<ChangeNumber> {
        match (self.change, self.change_id.as_deref()) {
            (Some(number), _) => Ok(number),
            (None, Some(id)) => resolve_change(client, id),
            (None, None) => bail!("pass --change <number> or --change-id <Change-Id>"),
        }
    }
}

/// The opt-in terse form (`--oneline`).
///
/// One whitespace-separated line per entry keyed by its global `sequence`.
pub(crate) fn print_oneline_entries(entries: &[LogEntry]) {
    for e in entries {
        println!(
            "sequence {}  {}  {}",
            e.sequence,
            e.payload.kind().as_str(),
            entry_summary(e)
        );
    }
}

/// One-line digest of a log entry.
///
/// A CLI display concern; the server ships only the raw entry.
fn entry_summary(entry: &LogEntry) -> String {
    let change = entry.change_number;
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
            Some(thread) => format!("author commented on thread {thread} (change {change})"),
            None => format!("author opened a thread on change {change}"),
        },
        LogPayload::Lifecycle(p) => format!("change {change} {}", p.action.as_str()),
        LogPayload::Tags(p) => format!("change {change} tagged ({})", p.tags.len()),
    }
}

/// The digest of the changes a tag selects.
///
/// Prints one `tag key=value` line per selecting tag, then one aligned
/// line per change: `number change_id status rN Nu subject`. The changes stay in
/// the server's order, ascending by change number. The number is the one
/// `nit comment --change` takes. `status` is the change's status at its
/// latest revision and `Nu` its unresolved threads over every revision.
pub(crate) fn tagged_digest(tags: &Tags, changes: &[ChangeProjection]) -> String {
    use std::fmt::Write;
    let (cells, subjects): (Vec<[String; 5]>, Vec<String>) = changes
        .iter()
        .map(|c| {
            let revision = c.latest_revision_number();
            let cells = [
                c.id.to_string(),
                short_change_id(&c.change_id),
                c.current_status().as_str().to_string(),
                format!("r{revision}"),
                format!("{}u", c.unresolved()),
            ];
            (cells, c.subject_at(revision))
        })
        .unzip();
    let inf = "write to String is infallible";
    let mut out = String::new();
    for tag in tags.spelled() {
        writeln!(out, "tag {tag}").expect(inf);
    }
    let widths = column_widths(&cells);
    for (cols, subject) in cells.iter().zip(&subjects) {
        writeln!(out, "{}", aligned_row(cols, widths, subject)).expect(inf);
    }
    out
}

/// The max display width of each column across `rows`, for space alignment.
pub(crate) fn column_widths<const N: usize>(rows: &[[String; N]]) -> [usize; N] {
    let mut widths = [0usize; N];
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    widths
}

/// One aligned row: the fixed cells, then the free-form `tail` field.
///
/// Each fixed cell is padded to its column width and two-space separated.
/// Shared by the change digest and the repo list.
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

pub(crate) fn short_change_id(change_id: &ChangeId) -> String {
    change_id.as_str().chars().take(8).collect()
}

/// Confirms a posted comment.
///
/// `opened thread N on change M  <anchor>  <state>` for a new thread, or
/// `replied on thread N (change M)  <state>` for a reply.
pub(crate) fn print_comment(thread: &ThreadProjection, change_number: ChangeNumber, replied: bool) {
    let state = if thread.resolved { "resolved" } else { "open" };
    if replied {
        println!(
            "replied on thread {} (change {change_number})  {state}",
            thread.id
        );
    } else {
        println!(
            "opened thread {} on change {change_number}  {}  {state}",
            thread.id,
            anchor_label(&thread.anchor),
        );
    }
}

/// The multi-line rendering of one log entry (no trailing blank line).
///
/// A pure function of that entry, so it reconstructs nothing the entry does
/// not carry: a `revision` entry shows no revision number, and a reply names
/// only its thread — a reply's anchor lives on the thread's opening entry.
fn render_entry(entry: &LogEntry) -> String {
    let sequence = entry.sequence;
    let change = entry.change_number;
    match &entry.payload {
        LogPayload::Revision(p) => format!(
            "sequence {sequence}  change {change}  revision {}  {}",
            short_sha(&p.commit_sha),
            subject_of(&p.message),
        ),
        LogPayload::Tags(p) => {
            let mut out = format!("sequence {sequence}  change {change}  tags");
            for (key, value) in p.tags.iter() {
                out.push('\n');
                out.push_str(&indent(&format!("{key}: {value}"), 4));
            }
            out
        }
        LogPayload::Review(p) => {
            let mut out = format!(
                "sequence {sequence}  change {change} r{}  reviewer: {}",
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
                Some(target) => {
                    format!("sequence {sequence}  change {change}  comment on {target}")
                }
                None => format!("sequence {sequence}  change {change}  comment"),
            };
            format!("{head}\n{}", indent(&c.body, 4))
        }
        LogPayload::Lifecycle(p) => match &p.message {
            Some(m) if !m.is_empty() => {
                format!(
                    "sequence {sequence}  change {change}  {}: {m}",
                    p.action.as_str()
                )
            }
            _ => format!(
                "sequence {sequence}  change {change}  {}",
                p.action.as_str()
            ),
        },
    }
}

/// One comment inside a `review`: `t<id>  <anchor>  [resolved]`.
///
/// The body follows on its own line indented one level deeper. The anchor
/// shows only when this entry opened the thread; a reply carries none.
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

/// The `thread N (location)` target of an author `comment` entry.
///
/// The location shows only when this entry opened the thread.
fn comment_target(c: &CommentInput) -> Option<String> {
    let tid = c.thread_id?;
    Some(match &c.anchor {
        Some(anchor) => format!("thread {tid} ({})", anchor_label(anchor)),
        None => format!("thread {tid}"),
    })
}

/// The `  <anchor>` an opening comment carries in its own payload.
///
/// Empty for a reply, whose anchor lives on the opening entry.
fn opening_anchor(c: &CommentInput) -> String {
    match &c.anchor {
        Some(anchor) => format!("  {}", anchor_label(anchor)),
        None => String::new(),
    }
}

/// The anchor label.
///
/// `(change-level)`, `file`, `file:line`, or the full
/// `file:start_line:start_char-end_line:end_char` for a selection. The log
/// renderer and the `nit comment` confirmation both use it.
fn anchor_label(anchor: &Anchor) -> String {
    match anchor {
        Anchor::Change => "(change-level)".to_string(),
        Anchor::File { file } => file.clone(),
        Anchor::Line {
            file,
            at: LineAnchor::Whole(line),
            ..
        } => format!("{file}:{line}"),
        Anchor::Line {
            file,
            at: LineAnchor::Selection(r),
            ..
        } => format!(
            "{file}:{}:{}-{}:{}",
            r.start_line(),
            r.start_char(),
            r.end_line(),
            r.end_char()
        ),
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

/// Prints each entry's rich rendering, a blank line between entries.
pub(crate) fn print_entries(entries: &[LogEntry]) {
    let text = render_entries(entries);
    if !text.is_empty() {
        println!("{text}");
    }
}

/// The full rendering of the entries, one blank line between them.
pub(crate) fn render_entries(entries: &[LogEntry]) -> String {
    entries
        .iter()
        .map(render_entry)
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nit_types::domain::RevisionNumber;

    use nit_types::domain::CommentRange;
    use nit_types::domain::LineAnchor;
    use nit_types::testing::{change_id, sha};

    #[test]
    fn entry_summary_digests_each_kind() {
        use nit_types::domain::{CommentInput, ReviewPayload, RevisionPayload};
        use nit_types::domain::{LifecycleAction, Verdict};
        let entry = |payload| LogEntry {
            change_number: ChangeNumber::new(7),
            position: 0,
            sequence: 0,
            created_at: String::new(),
            payload,
        };
        let comment = || CommentInput {
            thread_id: None,
            revision: None,
            anchor: None,
            body: String::new(),
            resolved: None,
        };
        let revision = entry(LogPayload::Revision(RevisionPayload {
            commit_sha: sha("abcdef0123456789"),
            parent_sha: sha(""),
            fork_sha: sha(""),
            message: String::new(),
            resets_status: true,
        }));
        assert_eq!(
            entry_summary(&revision),
            "change 7 new revision abcdef012345"
        );
        let review = entry(LogPayload::Review(ReviewPayload {
            revision: RevisionNumber::new(2),
            verdict: Verdict::RequestChanges,
            message: String::new(),
            comments: vec![comment(), comment()],
        }));
        assert_eq!(
            entry_summary(&review),
            "reviewer request_changes on change 7 r2 (2 comment(s))"
        );
        let opened = entry(LogPayload::Comment(comment()));
        assert_eq!(entry_summary(&opened), "author opened a thread on change 7");
        let life = entry(LogPayload::lifecycle(LifecycleAction::Merged, None, None));
        assert_eq!(entry_summary(&life), "change 7 merged");
    }

    #[test]
    fn log_render_review_and_revision() {
        use nit_types::domain::{CommentInput, ReviewPayload, RevisionPayload};
        use nit_types::domain::{Side, Verdict};
        let entry = |change_number, position, sequence, payload| LogEntry {
            change_number,
            position,
            sequence,
            created_at: String::new(),
            payload,
        };
        let opening = |tid, file: Option<&str>, at, resolved, body: &str| CommentInput {
            thread_id: Some(tid),
            revision: Some(RevisionNumber::new(2)),
            anchor: Some(
                Anchor::parse(file.map(String::from), Some(Side::New), at)
                    .expect("a fixture names one anchor"),
            ),
            body: body.to_string(),
            resolved,
        };
        let review = entry(
            ChangeNumber::new(42),
            5,
            12,
            LogPayload::Review(ReviewPayload {
                revision: RevisionNumber::new(2),
                verdict: Verdict::RequestChanges,
                message: "Cover one.\nCover two.".to_string(),
                comments: vec![
                    opening(3, None, None, None, "Change-level question?"),
                    opening(
                        4,
                        Some("src/queue.rs"),
                        Some(LineAnchor::Whole(42)),
                        None,
                        "Bounded channel.",
                    ),
                    opening(
                        5,
                        Some("src/queue.rs"),
                        Some(LineAnchor::Selection(
                            CommentRange::new(42, 8, 42, 30).expect("a forward range"),
                        )),
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
            "sequence 12  change 42 r2  reviewer: request_changes\n\
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
        let revision = |position, sequence, name: &str, msg: &str| {
            entry(
                ChangeNumber::new(42),
                position,
                sequence,
                LogPayload::Revision(RevisionPayload {
                    commit_sha: sha(name),
                    parent_sha: sha(""),
                    fork_sha: sha(""),
                    message: msg.to_string(),
                    resets_status: true,
                }),
            )
        };
        assert_eq!(
            render_entry(&revision(0, 3, "abcdef0123456789", "queue: first\n\nbody")),
            "sequence 3  change 42  revision abcdef012345  queue: first"
        );
        assert_eq!(
            render_entry(&revision(6, 20, "1234567890abcdef", "queue: second")),
            "sequence 20  change 42  revision 1234567890ab  queue: second"
        );
    }

    #[test]
    fn tagged_digest_lists_changes_by_number_at_their_latest_revision() {
        use nit_types::domain::{Lifecycle, RevisionProjection, ThreadProjection};
        use nit_types::testing::tags;
        let revision = |number: u64, message: &str| RevisionProjection {
            number: RevisionNumber::new(number),
            commit_sha: sha(""),
            parent_sha: sha(""),
            fork_sha: sha(""),
            message: message.to_string(),
            resets_status: true,
            created_at: String::new(),
        };
        let thread = |revision: u64, resolved: bool| ThreadProjection {
            id: 0,
            revision: RevisionNumber::new(revision),
            anchor: Anchor::Change,
            resolved,
            comments: vec![],
            created_at: String::new(),
            updated_at: String::new(),
        };
        let change = |id: u64, key: &str, revisions: Vec<RevisionProjection>, threads| {
            let mut c = ChangeProjection::new(ChangeNumber::new(id), 1, change_id(key));
            c.revisions = revisions;
            c.threads = threads;
            c
        };
        let mut merged = change(
            2,
            "Iabcdef0123456",
            vec![revision(0, "web: render")],
            vec![],
        );
        merged.lifecycle = Lifecycle::Merged;
        let changes = vec![
            merged,
            change(
                12,
                "I0123456789abc",
                vec![
                    revision(0, "old"),
                    revision(1, "server: add health\n\nbody"),
                ],
                vec![thread(0, false), thread(1, false), thread(1, true)],
            ),
        ];
        // The count spans every revision: the open thread on revision 0
        // counts with the one on revision 1.
        assert_eq!(
            tagged_digest(&tags(&[("branch", "track/a")]), &changes),
            "tag branch=track/a\n\
             2   Iabcdef0  merged   r0  0u  web: render\n\
             12  I0123456  pending  r1  2u  server: add health\n"
        );
    }
}
