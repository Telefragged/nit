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

/// A log entry to append, as its typed domain payload. The append primitive
/// (`crate::api::append_to_change`) derives the [`LogKind`] tag and serializes
/// the payload to its stored JSON itself, so callers never touch
/// `serde_json::Value` — that is the storage boundary's concern, not theirs.
#[derive(Debug, Clone)]
pub enum EntryPayload {
    Revision(RevisionPayload),
    Review(ReviewPayload),
    Comment(CommentPayload),
    Partial(PartialPayload),
    Lifecycle(LifecyclePayload),
}

impl EntryPayload {
    /// The kind tag this entry stores under.
    #[must_use]
    pub fn kind(&self) -> LogKind {
        match self {
            EntryPayload::Revision(_) => LogKind::Revision,
            EntryPayload::Review(_) => LogKind::Review,
            EntryPayload::Comment(_) => LogKind::Comment,
            EntryPayload::Partial(_) => LogKind::Partial,
            EntryPayload::Lifecycle(_) => LogKind::Lifecycle,
        }
    }

    /// Serialize the payload to its stored JSON shape.
    ///
    /// # Errors
    /// When the payload fails to serialize.
    pub fn to_value(&self) -> Result<serde_json::Value> {
        match self {
            EntryPayload::Revision(p) => serde_json::to_value(p),
            EntryPayload::Review(p) => serde_json::to_value(p),
            EntryPayload::Comment(p) => serde_json::to_value(p),
            EntryPayload::Partial(p) => serde_json::to_value(p),
            EntryPayload::Lifecycle(p) => serde_json::to_value(p),
        }
        .map_err(Into::into)
    }

    /// Parse a stored entry's `kind` + JSON `payload` back into the typed
    /// payload — the deserialize half of the storage boundary (mirrors
    /// [`to_value`](Self::to_value)).
    ///
    /// # Errors
    /// When the JSON does not match the payload shape for `kind`.
    fn from_json(kind: LogKind, json: &str) -> Result<EntryPayload> {
        fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> serde_json::Result<T> {
            serde_json::from_str(json)
        }
        Ok(match kind {
            LogKind::Revision => EntryPayload::Revision(parse(json)?),
            LogKind::Review => EntryPayload::Review(parse(json)?),
            LogKind::Comment => EntryPayload::Comment(parse(json)?),
            LogKind::Partial => EntryPayload::Partial(parse(json)?),
            LogKind::Lifecycle => EntryPayload::Lifecycle(parse(json)?),
        })
    }
}

// ---------------------------------------------------------------------------
// A folded log entry

/// One log entry as the fold sees it: coordinates plus the **typed** payload.
#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: u64,
    pub idx: u64,
    pub payload: EntryPayload,
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
            payload: EntryPayload::from_json(kind, &row.payload)
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

    /// Whether the latest revision is partial (`nit push --partial` set, not
    /// yet cleared by `nit ready`). A chain is partial iff its tip change is.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.latest_revision().is_some_and(|r| r.partial)
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
        EntryPayload::Revision(p) => fold_revision(change, p, &now),
        EntryPayload::Review(p) => {
            change.reviews.push(ReviewProj {
                id: p.review_id,
                revision: p.revision,
                verdict: p.verdict,
                message: p.message.clone(),
                created_at: now.clone(),
            });
            for c in &mut p.comments {
                change.mint_thread_id(c);
                apply_comment(change, c, Author::Reviewer, Some(p.review_id), &now);
            }
        }
        EntryPayload::Comment(p) => {
            change.mint_thread_id(&mut p.comment);
            apply_comment(change, &p.comment, Author::Agent, None, &now);
        }
        EntryPayload::Partial(p) => {
            if let Some(rev) = change.revisions.last_mut() {
                rev.partial = p.partial;
            }
        }
        EntryPayload::Lifecycle(p) => fold_lifecycle(change, p),
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
        partial: p.partial,
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
fn apply_comment(
    change: &mut ChangeProj,
    c: &CommentInput,
    author: Author,
    review_id: Option<u64>,
    now: &str,
) {
    let Some(tid) = c.thread_id else { return };
    if let Some(thread) = change.threads.iter_mut().find(|t| t.id == tid) {
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
    } else if !c.body.trim().is_empty() {
        open_thread(change, c, tid, author, review_id, now);
    }
}

/// Open a new thread carrying `id` at the comment's anchor. `next_thread_id` is
/// kept ahead by [`ChangeProj::mint_thread_id`], the sole owner of the counter.
fn open_thread(
    change: &mut ChangeProj,
    c: &CommentInput,
    id: u64,
    author: Author,
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
            author,
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
        if let EntryPayload::Review(p) = Entry::from_row(row)?.payload {
            max = max.max(p.review_id);
        }
    }
    Ok(max)
}

#[cfg(test)]
mod tests;
