//! The fold: a **change's** reviewable state is the replay of its
//! append-only event log (docs/data-model.md "The fold"). [`ChangeProj`] is
//! the in-memory state machine; [`fold`] applies one [`Entry`]; [`replay`]
//! rebuilds a change's projection from its row plus its log rows. A chain is
//! never folded — it is composed at read time from member projections
//! (`crate::chain`).
//!
//! Fold-assigned ids: review ids arrive already allocated inside the entry
//! payloads (the server mints them from a process-global counter at append
//! time). The change id is the `changes` rowid, carried on the projection.
//! Revision numbers (0-based) and thread ids are minted **in the fold** by
//! creation order — pure functions of the log, never stored (docs/data-model.md
//! "Identity"), so a concurrent shared-change push cannot mint a duplicate.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::db::{self, CommentRange};
use crate::enums::{Author, ChangeStatus, LifecycleAction, LogKind, Side, Verdict};

// ---------------------------------------------------------------------------
// Enums

impl From<Verdict> for ChangeStatus {
    /// The review status a verdict produces, before the lifecycle overlay
    /// (`merged`/`abandoned`) that [`ChangeProj::status_at`] layers on top
    /// (docs/data-model.md "The fold").
    fn from(verdict: Verdict) -> ChangeStatus {
        match verdict {
            Verdict::Approve => ChangeStatus::Approved,
            Verdict::RequestChanges => ChangeStatus::ChangesRequested,
            Verdict::Comment => ChangeStatus::Commented,
        }
    }
}

/// A change's terminal lifecycle, folded from its `lifecycle` entries
/// (docs/data-model.md "Lifecycle"). `Merged` records which patchset landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Active,
    Merged { revision: u64 },
    Abandoned,
}

// ---------------------------------------------------------------------------
// Log payloads (the JSON in each `log.payload`; docs/data-model.md "Payloads")

/// A `revision` entry: one new commit-sha observed for this change. The
/// revision `number` is **not** carried — the fold mints it (0-based, by
/// append order) so a concurrent shared-change push cannot duplicate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionPayload {
    pub commit_sha: String,
    pub parent_sha: String,
    pub base_sha: String,
    pub message: String,
    pub partial: bool,
    /// `false` only for a pure rebase (patch-id-equal, message unchanged): the
    /// new revision then inherits the prior revision's review status rather
    /// than resetting to `pending`.
    pub resets_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPayload {
    pub review_id: u64,
    pub revision: u64,
    pub verdict: Verdict,
    pub message: String,
    /// The drained drafts, in draft order. Each opens a new thread or replies
    /// to an existing one (see [`CommentInput`]).
    pub comments: Vec<CommentInput>,
}

/// The `comment` kind: one comment an agent posts, opening a thread or
/// continuing one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentPayload {
    #[serde(flatten)]
    pub comment: CommentInput,
}

/// A comment inside a `review` or `comment` payload: with `thread_id` unset it
/// **opens a new thread** anchored by the fields below; with it set it
/// **replies** to that thread (the anchor is ignored — the thread owns it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentInput {
    /// `None` opens a new thread; `Some` appends to that thread.
    #[serde(default)]
    pub thread_id: Option<u64>,
    /// Anchor revision for a new thread (a draft's own patchset — an interdiff
    /// old side pins to an earlier revision). The API always stamps it; the
    /// fold falls back to the change's latest only for a malformed payload.
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u64>,
    /// New-thread anchor side; `None` on a reply (the thread owns the anchor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    #[serde(default)]
    pub range: Option<CommentRange>,
    #[serde(default)]
    pub line_text: Option<String>,
    pub body: String,
    /// Thread-resolution decision (`Some(true/false)` = resolve/reopen, `None`
    /// = no decision). On a new thread it is the birth state; a `thread_id`
    /// reply with an empty `body` carries only this.
    #[serde(default)]
    pub resolved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialPayload {
    pub partial: bool,
}

/// A `lifecycle` entry: the merge timer (`merged`) and the `nit abandon` /
/// `nit reopen` actions. `revision` is set only for `merged` (which patchset
/// landed); `message` is an optional reason on `abandoned`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecyclePayload {
    pub action: LifecycleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// A parsed log entry

#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: u64,
    pub idx: u64,
    pub kind: LogKind,
    pub payload: serde_json::Value,
    pub created_at: String,
}

impl Entry {
    /// # Errors
    /// When the stored `kind` is unknown or the payload is not valid JSON.
    pub fn from_row(row: &db::LogRow) -> Result<Entry> {
        Ok(Entry {
            seq: row.seq,
            idx: row.idx,
            kind: row
                .kind
                .parse()
                .map_err(|e| anyhow!("log entry {}: {e}", row.idx))?,
            payload: serde_json::from_str(&row.payload)
                .map_err(|e| anyhow!("log entry {}: bad payload: {e}", row.idx))?,
            created_at: row.created_at.clone(),
        })
    }

    fn parse<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        serde_json::from_value(self.payload.clone()).map_err(|e| {
            anyhow!(
                "log entry {}: {} payload: {e}",
                self.idx,
                self.kind.as_str()
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Projection (the folded state of ONE change)

#[derive(Debug, Clone)]
pub struct RevisionProj {
    /// 0-based, minted in the fold.
    pub number: u64,
    pub commit_sha: String,
    pub parent_sha: String,
    pub base_sha: String,
    pub message: String,
    /// This push's partial flag; `nit ready` re-stamps the latest revision's.
    pub partial: bool,
    /// `false` for a pure rebase — the revision inherits the prior status.
    pub resets_status: bool,
    pub created_at: String,
}

/// Where a thread is anchored within a revision (docs/api.md "Comment
/// placement"), modeled so the invalid combinations the flat wire fields
/// allow are unrepresentable.
#[derive(Debug, Clone)]
pub enum Anchor {
    /// The change as a whole (no file).
    Change,
    /// A whole file (no line).
    File { file: String },
    /// A line, optionally a sub-line `range` selection within it.
    Line {
        file: String,
        side: Side,
        line: u64,
        line_text: Option<String>,
        range: Option<CommentRange>,
    },
}

impl Anchor {
    /// The anchor a new thread is born with, taken from its opening comment.
    fn from_input(c: &CommentInput) -> Anchor {
        match (&c.file, c.line) {
            (Some(file), Some(line)) => Anchor::Line {
                file: file.clone(),
                side: c.side.unwrap_or_default(),
                line,
                line_text: c.line_text.clone(),
                range: c.range,
            },
            (Some(file), None) => Anchor::File { file: file.clone() },
            (None, _) => Anchor::Change,
        }
    }
}

/// A located, resolvable conversation. Its anchor and birth come from its
/// first comment; the `id` is fold-assigned by creation order, never stored.
#[derive(Debug, Clone)]
pub struct ThreadProj {
    pub id: u64,
    pub revision: u64,
    pub anchor: Anchor,
    pub resolved: bool,
    pub comments: Vec<ThreadComment>,
    pub created_at: String,
    pub updated_at: String,
}

/// One message in a thread.
#[derive(Debug, Clone)]
pub struct ThreadComment {
    pub author: Author,
    pub body: String,
    pub review_id: Option<u64>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ReviewProj {
    pub id: u64,
    pub revision: u64,
    pub verdict: Verdict,
    pub message: String,
    pub created_at: String,
}

/// The fold of one change's log.
#[derive(Debug, Clone)]
pub struct ChangeProj {
    pub id: u64,
    pub repo_id: u64,
    pub change_key: String,
    pub created_at: String,
    pub revisions: Vec<RevisionProj>,
    pub threads: Vec<ThreadProj>,
    pub reviews: Vec<ReviewProj>,
    pub lifecycle: Lifecycle,
    /// The next thread id to mint — bumped each time a thread is opened.
    pub next_thread_id: u64,
    pub head: u64,
    pub last_entry_at: Option<String>,
}

impl ChangeProj {
    #[must_use]
    pub fn empty(row: &db::ChangeRow) -> ChangeProj {
        ChangeProj {
            id: row.id,
            repo_id: row.repo_id,
            change_key: row.change_key.clone(),
            created_at: row.created_at.clone(),
            revisions: Vec::new(),
            threads: Vec::new(),
            reviews: Vec::new(),
            lifecycle: Lifecycle::Active,
            next_thread_id: 0,
            head: 0,
            last_entry_at: None,
        }
    }

    #[must_use]
    pub fn updated_at(&self) -> &str {
        self.last_entry_at.as_deref().unwrap_or(&self.created_at)
    }

    #[must_use]
    pub fn latest_revision(&self) -> Option<&RevisionProj> {
        self.revisions.last()
    }

    #[must_use]
    pub fn revision(&self, number: u64) -> Option<&RevisionProj> {
        self.revisions.iter().find(|r| r.number == number)
    }

    #[must_use]
    pub fn thread(&self, id: u64) -> Option<&ThreadProj> {
        self.threads.iter().find(|t| t.id == id)
    }

    /// Unresolved threads anchored at `revision` (the count the reviewer owes).
    #[must_use]
    pub fn unresolved_at(&self, revision: u64) -> usize {
        self.threads
            .iter()
            .filter(|t| t.revision == revision && !t.resolved)
            .count()
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        !matches!(self.lifecycle, Lifecycle::Active)
    }

    /// Whether the change has **landed** on the canonical branch. Distinct from
    /// `is_terminal`: an abandoned change is terminal but not merged, and stays
    /// an enumerable member/tip of its chains (abandonment is membership-inert).
    #[must_use]
    pub fn is_merged(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Merged { .. })
    }

    /// Whether the latest revision is partial (`nit push --partial` set, not
    /// yet cleared by `nit ready`). A chain is partial iff its tip change is.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.latest_revision().is_some_and(|r| r.partial)
    }

    /// The displayed status at a pinned revision: the lifecycle overlay
    /// (`abandoned` change-wide, `merged` only for the landed patchset) over
    /// the verdict-derived review status (docs/data-model.md "Per-change,
    /// per-revision status").
    #[must_use]
    pub fn status_at(&self, revision: u64) -> ChangeStatus {
        if matches!(self.lifecycle, Lifecycle::Abandoned) {
            return ChangeStatus::Abandoned;
        }
        if let Lifecycle::Merged { revision: landed } = self.lifecycle
            && landed == revision
        {
            return ChangeStatus::Merged;
        }
        self.review_status_at(revision)
    }

    /// The verdict-derived status at a revision: the latest review on it, else
    /// the prior revision's status when this one is a pure rebase, else
    /// pending. Never the lifecycle-overlay values (`merged`/`abandoned`).
    fn review_status_at(&self, revision: u64) -> ChangeStatus {
        if let Some(rv) = self
            .reviews
            .iter()
            .filter(|r| r.revision == revision)
            .max_by_key(|r| r.id)
        {
            return rv.verdict.into();
        }
        // No review here: a pure-rebase revision carries the prior one forward.
        if revision > 0 && self.revision(revision).is_some_and(|r| !r.resets_status) {
            return self.review_status_at(revision - 1);
        }
        ChangeStatus::Pending
    }
}

// ---------------------------------------------------------------------------
// Fold

/// Apply one entry to a change's projection (docs/data-model.md "The fold").
///
/// # Errors
/// When a payload fails to parse.
pub fn fold(change: &mut ChangeProj, entry: &Entry) -> Result<()> {
    match entry.kind {
        LogKind::Revision => fold_revision(change, &entry.parse()?, &entry.created_at),
        LogKind::Review => fold_review(change, &entry.parse()?, &entry.created_at),
        LogKind::Comment => {
            let p: CommentPayload = entry.parse()?;
            apply_comment(change, &p.comment, Author::Agent, None, &entry.created_at);
        }
        LogKind::Partial => {
            let p: PartialPayload = entry.parse()?;
            if let Some(rev) = change.revisions.last_mut() {
                rev.partial = p.partial;
            }
        }
        LogKind::Lifecycle => fold_lifecycle(change, &entry.parse()?),
    }
    change.head = entry.idx + 1;
    change.last_entry_at = Some(entry.created_at.clone());
    Ok(())
}

fn fold_revision(change: &mut ChangeProj, p: &RevisionPayload, now: &str) {
    let number = u64::try_from(change.revisions.len()).expect("revision count fits u64");
    change.revisions.push(RevisionProj {
        number,
        commit_sha: p.commit_sha.clone(),
        parent_sha: p.parent_sha.clone(),
        base_sha: p.base_sha.clone(),
        message: p.message.clone(),
        partial: p.partial,
        resets_status: p.resets_status,
        created_at: now.to_string(),
    });
}

fn fold_review(change: &mut ChangeProj, p: &ReviewPayload, now: &str) {
    change.reviews.push(ReviewProj {
        id: p.review_id,
        revision: p.revision,
        verdict: p.verdict,
        message: p.message.clone(),
        created_at: now.to_string(),
    });
    for c in &p.comments {
        apply_comment(change, c, Author::Reviewer, Some(p.review_id), now);
    }
}

fn fold_lifecycle(change: &mut ChangeProj, p: &LifecyclePayload) {
    change.lifecycle = match p.action {
        LifecycleAction::Merged => Lifecycle::Merged {
            revision: p.revision.unwrap_or(0),
        },
        LifecycleAction::Abandoned => Lifecycle::Abandoned,
        LifecycleAction::Reopened => Lifecycle::Active,
    };
}

/// Apply one comment to a change's threads (shared by `review` and `comment`).
/// With no `thread_id`, mint the next thread id and open a thread at the
/// comment's anchor; with one, append to that thread. An empty body adds no
/// comment, only its resolution (docs/data-model.md "The fold").
fn apply_comment(
    change: &mut ChangeProj,
    c: &CommentInput,
    author: Author,
    review_id: Option<u64>,
    now: &str,
) {
    match c.thread_id {
        None => {
            // A new thread needs a body; never mint an id for an empty one.
            if c.body.trim().is_empty() {
                return;
            }
            let id = change.next_thread_id;
            let revision = c
                .revision
                .unwrap_or_else(|| change.latest_revision().map_or(0, |r| r.number));
            change.threads.push(ThreadProj {
                id,
                revision,
                anchor: Anchor::from_input(c),
                resolved: c.resolved.unwrap_or(false),
                comments: vec![ThreadComment {
                    author,
                    body: c.body.clone(),
                    review_id,
                    created_at: now.to_string(),
                }],
                created_at: now.to_string(),
                updated_at: now.to_string(),
            });
            change.next_thread_id += 1;
        }
        Some(tid) => {
            let Some(thread) = change.threads.iter_mut().find(|t| t.id == tid) else {
                return;
            };
            if !c.body.trim().is_empty() {
                thread.comments.push(ThreadComment {
                    author,
                    body: c.body.clone(),
                    review_id,
                    created_at: now.to_string(),
                });
            }
            if let Some(state) = c.resolved {
                thread.resolved = state;
            }
            thread.updated_at = now.to_string();
        }
    }
}

/// Rebuild a change's projection from its row and its log rows (ascending idx).
///
/// # Errors
/// When a log payload fails to parse.
pub fn replay(row: &db::ChangeRow, rows: &[db::LogRow]) -> Result<ChangeProj> {
    let mut change = ChangeProj::empty(row);
    for log_row in rows {
        let entry = Entry::from_row(log_row)?;
        fold(&mut change, &entry)?;
    }
    Ok(change)
}

/// The maximum fold-assigned id (review ids) in a batch of log rows — used to
/// resume the global id counter on startup (docs/data-model.md "Identity").
///
/// # Errors
/// When a payload fails to parse.
pub fn max_assigned_id(rows: &[db::LogRow]) -> Result<u64> {
    let mut max = 0;
    for row in rows {
        let entry = Entry::from_row(row)?;
        if entry.kind == LogKind::Review {
            max = max.max(entry.parse::<ReviewPayload>()?.review_id);
        }
    }
    Ok(max)
}

#[cfg(test)]
mod tests;
