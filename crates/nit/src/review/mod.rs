//! The fold: a chain's reviewable state is the replay of its append-only
//! event log (docs/data-model.md "The fold"). [`Projection`] is the
//! in-memory state machine; [`fold`] applies one [`Entry`]; [`replay`]
//! rebuilds a projection from a chain row plus its log rows.
//!
//! Fold-assigned ids: change and review ids arrive already allocated inside
//! the entry payloads (the server mints them from a process-global counter at
//! append time and writes them in, so replay just trusts them). Thread ids are
//! **not** stored — a thread is numbered by its creation order as the fold
//! replays, a pure function of the log (docs/data-model.md "Identity").

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::db::{self, CommentRange};
use crate::enums::{Author, ChainState, ChangeStatus, ClosedStatus, LogKind, Side, Verdict};

// ---------------------------------------------------------------------------
// Enums

/// A chain's lifecycle status — also the wire `Chain.status` (docs/api.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainStatus {
    Active,
    Merged,
    Abandoned,
}

impl From<ClosedStatus> for ChainStatus {
    fn from(closed: ClosedStatus) -> ChainStatus {
        match closed {
            ClosedStatus::Merged => ChainStatus::Merged,
            ClosedStatus::Abandoned => ChainStatus::Abandoned,
        }
    }
}

/// A change's retained review status — never `orphaned` (that is the
/// separate [`ChangeProj::orphaned`] flag; the wire [`ChangeStatus`] is
/// `orphaned` while it is set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Approved,
    ChangesRequested,
    Commented,
}

impl From<Verdict> for Status {
    /// The status a verdict produces (docs/data-model.md "The fold").
    fn from(verdict: Verdict) -> Status {
        match verdict {
            Verdict::Approve => Status::Approved,
            Verdict::RequestChanges => Status::ChangesRequested,
            Verdict::Comment => Status::Commented,
        }
    }
}

impl From<Status> for ChangeStatus {
    /// The wire status of a non-orphaned change (orphaned is handled by
    /// [`ChangeProj::wire_status`]).
    fn from(status: Status) -> ChangeStatus {
        match status {
            Status::Pending => ChangeStatus::Pending,
            Status::Approved => ChangeStatus::Approved,
            Status::ChangesRequested => ChangeStatus::ChangesRequested,
            Status::Commented => ChangeStatus::Commented,
        }
    }
}

// ---------------------------------------------------------------------------
// Log payloads (the JSON in each `log.payload`; docs/data-model.md "Payloads")

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionsPayload {
    pub live: Vec<LivePos>,
    pub added: Vec<AddedRevision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivePos {
    pub change_key: String,
    pub change_id: u64,
    pub position: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddedRevision {
    pub change_key: String,
    pub number: u64,
    pub commit_sha: String,
    pub parent_sha: String,
    pub message: String,
    pub resets_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPayload {
    pub change_key: String,
    pub review_id: u64,
    pub revision: u64,
    pub verdict: Verdict,
    pub message: String,
    /// The drained drafts, in draft order. Each opens a new thread or replies
    /// to an existing one (see [`CommentInput`]).
    pub comments: Vec<CommentInput>,
}

/// The `comment` kind: one comment an agent posts, opening a thread or
/// continuing one. The agent-authored mirror of a single review comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentPayload {
    pub change_key: String,
    #[serde(flatten)]
    pub comment: CommentInput,
}

/// A comment inside a `review` or `comment` payload: with `thread_id` unset it
/// **opens a new thread** anchored by the fields below; with it set it
/// **replies** to that thread (the anchor is ignored — the thread owns it).
/// Shared by both kinds (docs/data-model.md "Payloads").
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainClosedPayload {
    pub status: ClosedStatus,
}

// ---------------------------------------------------------------------------
// A parsed log entry

#[derive(Debug, Clone)]
pub struct Entry {
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
// Projection (the folded state)

#[derive(Debug, Clone)]
pub struct RevisionProj {
    pub number: u64,
    pub commit_sha: String,
    pub parent_sha: String,
    pub message: String,
    pub created_at: String,
}

/// Where a thread is anchored within a revision (docs/api.md "Comment
/// placement"), modeled so the invalid combinations the flat wire fields
/// allow are unrepresentable: a `range` cannot exist without its `line`, a
/// `line` not without its `file`, and `side`/`line_text` are meaningful only
/// at a line. The flat `file`/`line`/`side`/`range`/`line_text` of the wire
/// [`Thread`](crate::api::types::Thread) are this projected back out.
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
        /// Best-effort snapshot of the anchored line's text.
        line_text: Option<String>,
        range: Option<CommentRange>,
    },
}

impl Anchor {
    /// The anchor a new thread is born with, taken from its opening comment.
    /// `file` without `line` is a file-level anchor; no `file` is
    /// change-level (the API rejects a `line` without a `file` upstream).
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
/// first comment; later comments only extend it and may move `resolved`. The
/// `id` is fold-assigned by creation order, never stored (module docs).
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
    /// The review that published this comment; `None` for an agent comment.
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

#[derive(Debug, Clone)]
pub struct ChangeProj {
    pub id: u64,
    pub change_key: String,
    pub position: Option<u64>,
    pub orphaned: bool,
    pub status: Status,
    pub revisions: Vec<RevisionProj>,
    pub threads: Vec<ThreadProj>,
    pub reviews: Vec<ReviewProj>,
}

impl ChangeProj {
    #[must_use]
    pub fn latest_revision(&self) -> Option<&RevisionProj> {
        self.revisions.last()
    }

    /// The wire status: `orphaned` while orphaned, else the retained status.
    #[must_use]
    pub fn wire_status(&self) -> ChangeStatus {
        if self.orphaned {
            ChangeStatus::Orphaned
        } else {
            self.status.into()
        }
    }

    #[must_use]
    pub fn revision(&self, number: u64) -> Option<&RevisionProj> {
        self.revisions.iter().find(|r| r.number == number)
    }

    /// A published thread by its fold id.
    #[must_use]
    pub fn thread(&self, id: u64) -> Option<&ThreadProj> {
        self.threads.iter().find(|t| t.id == id)
    }

    /// Count of unresolved threads (open conversations awaiting the reviewer).
    #[must_use]
    pub fn unresolved_threads(&self) -> usize {
        self.threads.iter().filter(|t| !t.resolved).count()
    }

    /// The latest review on the change (highest id), if any.
    #[must_use]
    pub fn latest_review(&self) -> Option<&ReviewProj> {
        self.reviews.iter().max_by_key(|r| r.id)
    }

    /// The highest revision number that carries a review, else `None`.
    #[must_use]
    pub fn last_reviewed_revision(&self) -> Option<u64> {
        self.reviews.iter().map(|r| r.revision).max()
    }
}

#[derive(Debug, Clone)]
pub struct Projection {
    pub chain_id: u64,
    pub repo_id: u64,
    /// The owning repo's git-common-dir — the path every git operation opens.
    pub git_dir: String,
    pub branch: String,
    pub base: String,
    pub created_at: String,
    pub status: ChainStatus,
    pub partial: bool,
    pub changes: Vec<ChangeProj>,
    /// The next thread id to mint — bumped each time a thread is opened during
    /// the fold, so ids are positional and never stored (module docs).
    pub next_thread_id: u64,
    pub head: u64,
    pub last_entry_at: Option<String>,
    // Transient scan state — not folded from the log, re-derived each scan.
    pub last_scan_error: Option<String>,
    pub branch_missing_since: Option<String>,
}

impl Projection {
    #[must_use]
    pub fn empty(chain: &db::ChainRow) -> Projection {
        Projection {
            chain_id: chain.id,
            repo_id: chain.repo_id,
            git_dir: chain.git_dir.clone(),
            branch: chain.branch.clone(),
            base: chain.base.clone(),
            created_at: chain.created_at.clone(),
            status: ChainStatus::Active,
            partial: false,
            changes: Vec::new(),
            next_thread_id: 0,
            head: 0,
            last_entry_at: None,
            last_scan_error: None,
            branch_missing_since: None,
        }
    }

    /// `updated_at` = the last entry's time, else the chain's creation time.
    #[must_use]
    pub fn updated_at(&self) -> &str {
        self.last_entry_at.as_deref().unwrap_or(&self.created_at)
    }

    fn change_by_key_mut(&mut self, key: &str) -> Option<&mut ChangeProj> {
        self.changes.iter_mut().find(|c| c.change_key == key)
    }

    #[must_use]
    pub fn change_by_key(&self, key: &str) -> Option<&ChangeProj> {
        self.changes.iter().find(|c| c.change_key == key)
    }

    #[must_use]
    pub fn change_by_id(&self, id: u64) -> Option<&ChangeProj> {
        self.changes.iter().find(|c| c.id == id)
    }

    /// Live changes (not orphaned) in chain order; then orphans last —
    /// matching the dashboard ordering.
    #[must_use]
    pub fn changes_ordered(&self) -> Vec<&ChangeProj> {
        let mut live: Vec<&ChangeProj> = self.changes.iter().filter(|c| !c.orphaned).collect();
        live.sort_by_key(|c| c.position.unwrap_or(u64::MAX));
        let mut orphans: Vec<&ChangeProj> = self.changes.iter().filter(|c| c.orphaned).collect();
        live.append(&mut orphans);
        live
    }
}

// ---------------------------------------------------------------------------
// Fold

/// Apply one entry to the projection (docs/data-model.md "The fold").
///
/// # Errors
/// When a payload fails to parse.
pub fn fold(proj: &mut Projection, entry: &Entry) -> Result<()> {
    match entry.kind {
        LogKind::Revisions => fold_revisions(proj, &entry.parse()?, &entry.created_at),
        LogKind::Review => fold_review(proj, &entry.parse()?, &entry.created_at),
        LogKind::Comment => fold_comment(proj, &entry.parse()?, &entry.created_at),
        LogKind::Partial => proj.partial = entry.parse::<PartialPayload>()?.partial,
        LogKind::ChainClosed => {
            proj.status = entry.parse::<ChainClosedPayload>()?.status.into();
        }
    }
    proj.head = entry.idx + 1;
    proj.last_entry_at = Some(entry.created_at.clone());
    Ok(())
}

fn fold_revisions(proj: &mut Projection, p: &RevisionsPayload, now: &str) {
    // A revisions entry means the branch is alive with commits.
    proj.status = ChainStatus::Active;
    let live_keys: std::collections::HashSet<&str> =
        p.live.iter().map(|l| l.change_key.as_str()).collect();

    // Establish the live set (create new changes, set positions, un-orphan).
    for l in &p.live {
        if let Some(change) = proj.change_by_key_mut(&l.change_key) {
            change.position = Some(l.position);
            change.orphaned = false;
        } else {
            proj.changes.push(ChangeProj {
                id: l.change_id,
                change_key: l.change_key.clone(),
                position: Some(l.position),
                orphaned: false,
                status: Status::Pending,
                revisions: Vec::new(),
                threads: Vec::new(),
                reviews: Vec::new(),
            });
        }
    }
    // Orphan changes that left the walk (state retained, position cleared).
    for change in &mut proj.changes {
        if !live_keys.contains(change.change_key.as_str()) {
            change.orphaned = true;
            change.position = None;
        }
    }
    // Append new revisions; a non-pure-rebase revision resets review status.
    for a in &p.added {
        if let Some(change) = proj.change_by_key_mut(&a.change_key) {
            change.revisions.push(RevisionProj {
                number: a.number,
                commit_sha: a.commit_sha.clone(),
                parent_sha: a.parent_sha.clone(),
                message: a.message.clone(),
                created_at: now.to_string(),
            });
            if a.resets_status {
                change.status = Status::Pending;
            }
        }
    }
}

fn fold_review(proj: &mut Projection, p: &ReviewPayload, now: &str) {
    let Some(change) = proj.change_by_key_mut(&p.change_key) else {
        return;
    };
    change.reviews.push(ReviewProj {
        id: p.review_id,
        revision: p.revision,
        verdict: p.verdict,
        message: p.message.clone(),
        created_at: now.to_string(),
    });
    // Each drained draft opens or continues a thread, authored by the reviewer
    // and tagged with this review. Resolutions apply in draft order, so a
    // thread ends at its last decision (docs/data-model.md "The fold").
    for c in &p.comments {
        apply_comment(
            proj,
            &p.change_key,
            c,
            Author::Reviewer,
            Some(p.review_id),
            now,
        );
    }
    if let Some(change) = proj.change_by_key_mut(&p.change_key) {
        change.status = Status::from(p.verdict);
    }
}

/// Fold an agent `comment`: one comment authored by the agent, opening or
/// continuing a thread with no review attached (the change's status is
/// untouched — an agent's note is not a verdict).
fn fold_comment(proj: &mut Projection, p: &CommentPayload, now: &str) {
    apply_comment(proj, &p.change_key, &p.comment, Author::Agent, None, now);
}

/// Apply one comment to a change's threads (shared by `review` and `comment`).
/// With no `thread_id`, mint the next thread id and open a thread at the
/// comment's anchor; with one, append to that thread. An empty body adds no
/// comment, only its resolution (docs/data-model.md "The fold").
fn apply_comment(
    proj: &mut Projection,
    change_key: &str,
    c: &CommentInput,
    author: Author,
    review_id: Option<u64>,
    now: &str,
) {
    match c.thread_id {
        None => {
            // A new thread needs a body; never mint an id for an empty one, so
            // the counter stays a function of the threads actually created.
            if c.body.trim().is_empty() {
                return;
            }
            let id = proj.next_thread_id;
            let Some(change) = proj.change_by_key_mut(change_key) else {
                return;
            };
            // The API always stamps `revision`; fall back to latest only for a
            // malformed payload.
            let revision = c
                .revision
                .unwrap_or_else(|| change.latest_revision().map_or(1, |r| r.number));
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
            proj.next_thread_id += 1;
        }
        Some(tid) => {
            let Some(change) = proj.change_by_key_mut(change_key) else {
                return;
            };
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
            // A reply always changes something — the API forbids an empty body
            // with no resolution — so bump the thread's time unconditionally.
            thread.updated_at = now.to_string();
        }
    }
}

/// Rebuild a projection from a chain row and its log rows (ascending idx).
///
/// # Errors
/// When a log payload fails to parse.
pub fn replay(chain: &db::ChainRow, rows: &[db::LogRow]) -> Result<Projection> {
    let mut proj = Projection::empty(chain);
    for row in rows {
        let entry = Entry::from_row(row)?;
        fold(&mut proj, &entry)?;
    }
    Ok(proj)
}

/// The maximum fold-assigned id appearing in a batch of log rows — used to
/// resume the global id counter on startup (docs/data-model.md "Identity").
///
/// # Errors
/// When a payload fails to parse.
pub fn max_assigned_id(rows: &[db::LogRow]) -> Result<u64> {
    let mut max = 0;
    for row in rows {
        let entry = Entry::from_row(row)?;
        match entry.kind {
            LogKind::Revisions => {
                for l in entry.parse::<RevisionsPayload>()?.live {
                    max = max.max(l.change_id);
                }
            }
            LogKind::Review => {
                max = max.max(entry.parse::<ReviewPayload>()?.review_id);
            }
            LogKind::Comment | LogKind::Partial | LogKind::ChainClosed => {}
        }
    }
    Ok(max)
}

// ---------------------------------------------------------------------------
// Derived chain state + wake rule

/// Derived chain state (docs/data-model.md "Derived chain state").
#[must_use]
pub fn derive_state(proj: &Projection) -> ChainState {
    match proj.status {
        ChainStatus::Merged => ChainState::Merged,
        ChainStatus::Abandoned => ChainState::Abandoned,
        ChainStatus::Active => {
            let live: Vec<&ChangeProj> = proj.changes.iter().filter(|c| !c.orphaned).collect();
            if live.is_empty() {
                return ChainState::AgentsTurn; // empty chain
            }
            if live
                .iter()
                .any(|c| matches!(c.status, Status::ChangesRequested | Status::Commented))
            {
                ChainState::AgentsTurn
            } else if live.iter().any(|c| c.status != Status::Approved) {
                ChainState::WaitingForReview
            } else if proj.partial {
                ChainState::AgentsTurn
            } else {
                ChainState::Approved
            }
        }
    }
}

#[cfg(test)]
mod tests;
