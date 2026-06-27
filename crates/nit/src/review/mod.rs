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
//! Revision numbers (0-based) are minted **in the fold** by creation order — a
//! pure function of the log, never stored. Thread ids are minted in the fold
//! too: [`fold`] takes an entry by value and, via
//! [`ChangeProj::mint_thread_id`], fills a new-thread comment's `thread_id` from
//! `next_thread_id` and returns the entry with the id written into its payload,
//! so the caller stores and broadcasts that one value. `next_thread_id` is the
//! single source of truth — the only field minting touches — so a concurrent
//! shared-change push can't duplicate an id, and replay (ids already set) just
//! advances it (docs/data-model.md "Identity").

use anyhow::{Result, anyhow};
use nit_types::comments::CommentRange;

use crate::db;
use crate::enums::{ChangeStatus, LifecycleAction, LogKind, Side, Verdict};

// ---------------------------------------------------------------------------
// Enums

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
pub use nit_types::log::{
    CommentInput, LifecyclePayload, LogPayload, ReviewPayload, RevisionPayload,
};

/// Serialize a log payload to the JSON stored in its `log.payload` column: the
/// inner struct alone, since the entry's `kind` is stored in its own column.
/// The write half of the storage boundary ([`payload_from_json`] is the read
/// half) — no `serde_json::Value` crosses it.
///
/// # Errors
/// When the payload fails to serialize — impossible for these plain structs.
pub(crate) fn payload_to_json(payload: &LogPayload) -> Result<String> {
    match payload {
        LogPayload::Revision(p) => serde_json::to_string(p),
        LogPayload::Review(p) => serde_json::to_string(p),
        LogPayload::Comment(p) => serde_json::to_string(p),
        LogPayload::Lifecycle(p) => serde_json::to_string(p),
    }
    .map_err(Into::into)
}

/// Parse a stored entry's `kind` + inner JSON back into the typed payload — the
/// read half of the storage boundary (mirrors [`payload_to_json`]).
///
/// # Errors
/// When the JSON does not match the payload shape for `kind`.
pub(crate) fn payload_from_json(kind: LogKind, json: &str) -> Result<LogPayload> {
    Ok(match kind {
        LogKind::Revision => LogPayload::Revision(serde_json::from_str(json)?),
        LogKind::Review => LogPayload::Review(serde_json::from_str(json)?),
        LogKind::Comment => LogPayload::Comment(serde_json::from_str(json)?),
        LogKind::Lifecycle => LogPayload::Lifecycle(serde_json::from_str(json)?),
    })
}

// ---------------------------------------------------------------------------
// A folded log entry

/// One log entry as the fold sees it: coordinates plus the **typed** payload.
#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: u64,
    pub idx: u64,
    pub payload: LogPayload,
    pub created_at: String,
}

impl Entry {
    /// # Errors
    /// When the stored `kind` is unknown or the payload is not valid JSON.
    pub fn from_row(row: &db::LogRow) -> Result<Entry> {
        let kind: LogKind = row
            .kind
            .parse()
            .map_err(|e| anyhow!("log entry {}: {e}", row.idx))?;
        Ok(Entry {
            seq: row.seq,
            idx: row.idx,
            payload: payload_from_json(kind, &row.payload)
                .map_err(|e| anyhow!("log entry {}: bad payload: {e}", row.idx))?,
            created_at: row.created_at.clone(),
        })
    }

    /// This entry's kind, from its payload variant.
    #[must_use]
    pub fn kind(&self) -> LogKind {
        self.payload.kind()
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

/// One message in a thread. `review_id` is the review that published it, or
/// `None` for an agent's own note — which is what distinguishes reviewer from
/// agent (the only consumer derives the label from it).
#[derive(Debug, Clone)]
pub struct ThreadComment {
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
        }
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

    /// The change's current status: [`status_at`](Self::status_at) its latest
    /// revision (pending when it has none). The denormalized `changes.status`
    /// column caches this so a query can filter changes without folding their
    /// logs (docs/data-model.md "Tables").
    #[must_use]
    pub fn current_status(&self) -> ChangeStatus {
        self.status_at(self.latest_revision().map_or(0, |r| r.number))
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

    /// Resolve a comment's thread id and keep `next_thread_id` — the single
    /// source of truth — past it (docs/data-model.md "Identity"). A new-thread
    /// comment (no id, real body) is minted the next id; any id then bumps the
    /// counter. The fold calls this before applying each comment, so a live
    /// append mints (and the stored payload then carries the id) while replay,
    /// seeing the id already set, only advances the counter — no double count.
    fn mint_thread_id(&mut self, comment: &mut CommentInput) {
        if comment.thread_id.is_none() && !comment.body.trim().is_empty() {
            comment.thread_id = Some(self.next_thread_id);
        }
        if let Some(id) = comment.thread_id {
            self.next_thread_id = self.next_thread_id.max(id + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Fold

/// Apply one entry to a change's projection (docs/data-model.md "The fold"),
/// minting any new-thread ids into the entry's typed payload and returning the
/// id-bearing entry.
pub fn fold(change: &mut ChangeProj, mut entry: Entry) -> Entry {
    let now = entry.created_at.clone();
    match &mut entry.payload {
        LogPayload::Revision(p) => fold_revision(change, p, &now),
        LogPayload::Review(p) => {
            change.reviews.push(ReviewProj {
                id: p.review_id,
                revision: p.revision,
                verdict: p.verdict,
                message: p.message.clone(),
                created_at: now.clone(),
            });
            for c in &mut p.comments {
                change.mint_thread_id(c);
                apply_comment(change, c, Some(p.review_id), &now);
            }
        }
        LogPayload::Comment(c) => {
            change.mint_thread_id(c);
            apply_comment(change, c, None, &now);
        }
        LogPayload::Lifecycle(p) => fold_lifecycle(change, p),
    }
    entry
}

fn fold_revision(change: &mut ChangeProj, p: &RevisionPayload, now: &str) {
    let number = u64::try_from(change.revisions.len()).expect("revision count fits u64");
    change.revisions.push(RevisionProj {
        number,
        commit_sha: p.commit_sha.clone(),
        parent_sha: p.parent_sha.clone(),
        base_sha: p.base_sha.clone(),
        message: p.message.clone(),
        resets_status: p.resets_status,
        created_at: now.to_string(),
    });
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

/// Apply one comment — its `thread_id` already resolved by
/// [`ChangeProj::mint_thread_id`] — to a change's threads (shared by `review`
/// and `comment`; docs/data-model.md "The fold"):
///   - **the thread already exists** — append to it (a reply); an empty body
///     carries only its resolution.
///   - **a set id not seen yet** — open the thread it names.
///   - **unset** — an empty new thread the mint left alone; a no-op.
fn apply_comment(change: &mut ChangeProj, c: &CommentInput, review_id: Option<u64>, now: &str) {
    let Some(tid) = c.thread_id else { return };
    if let Some(thread) = change.threads.iter_mut().find(|t| t.id == tid) {
        if !c.body.trim().is_empty() {
            thread.comments.push(ThreadComment {
                body: c.body.clone(),
                review_id,
                created_at: now.to_string(),
            });
        }
        if let Some(state) = c.resolved {
            thread.resolved = state;
        }
        thread.updated_at = now.to_string();
    } else if !c.body.trim().is_empty() {
        open_thread(change, c, tid, review_id, now);
    }
}

/// Open a new thread carrying `id` at the comment's anchor. `next_thread_id` is
/// kept ahead by [`ChangeProj::mint_thread_id`], the sole owner of the counter.
fn open_thread(
    change: &mut ChangeProj,
    c: &CommentInput,
    id: u64,
    review_id: Option<u64>,
    now: &str,
) {
    let revision = c
        .revision
        .unwrap_or_else(|| change.latest_revision().map_or(0, |r| r.number));
    change.threads.push(ThreadProj {
        id,
        revision,
        anchor: Anchor::from_input(c),
        resolved: c.resolved.unwrap_or(false),
        comments: vec![ThreadComment {
            body: c.body.clone(),
            review_id,
            created_at: now.to_string(),
        }],
        created_at: now.to_string(),
        updated_at: now.to_string(),
    });
}

/// Rebuild a change's projection from its row and its log rows (ascending idx).
///
/// # Errors
/// When a log payload fails to parse.
pub fn replay(row: &db::ChangeRow, rows: &[db::LogRow]) -> Result<ChangeProj> {
    let mut change = ChangeProj::empty(row);
    for log_row in rows {
        fold(&mut change, Entry::from_row(log_row)?);
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
        if let LogPayload::Review(p) = Entry::from_row(row)?.payload {
            max = max.max(p.review_id);
        }
    }
    Ok(max)
}

#[cfg(test)]
mod tests;
