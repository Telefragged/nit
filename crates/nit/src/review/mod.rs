//! The fold: a chain's reviewable state is the replay of its append-only
//! event log (docs/data-model.md "The fold"). [`Projection`] is the
//! in-memory state machine; [`fold`] applies one [`Entry`]; [`replay`]
//! rebuilds a projection from a chain row plus its log rows.
//!
//! Fold-assigned ids (changes, published comments, reviews) arrive already
//! allocated inside the entry payloads — the server allocates them from a
//! process-global counter at append time and writes them in, so replay just
//! trusts them (docs/data-model.md "Identity within the log").

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::db::{self, CommentRange};

// ---------------------------------------------------------------------------
// Enums

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStatus {
    Active,
    Merged,
    Abandoned,
}

impl ChainStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChainStatus::Active => "active",
            ChainStatus::Merged => "merged",
            ChainStatus::Abandoned => "abandoned",
        }
    }
}

/// A change's retained review status — never `orphaned` (that is the
/// separate [`ChangeProj::orphaned`] flag; the wire status is "orphaned"
/// while it is set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Approved,
    ChangesRequested,
    Commented,
}

impl Status {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Approved => "approved",
            Status::ChangesRequested => "changes_requested",
            Status::Commented => "commented",
        }
    }

    /// The status a verdict produces (docs/data-model.md "The fold").
    #[must_use]
    pub fn from_verdict(verdict: &str) -> Status {
        match verdict {
            "approve" => Status::Approved,
            "request_changes" => Status::ChangesRequested,
            "comment" => Status::Commented,
            _ => Status::Pending,
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
    pub verdict: String,
    pub message: String,
    pub comments: Vec<PublishedComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedComment {
    pub id: u64,
    pub parent_id: Option<u64>,
    /// The revision the comment was authored on (a draft's own revision —
    /// not necessarily the review's target). `None` only on log entries that
    /// predate this field; the fold then falls back to the review revision.
    #[serde(default)]
    pub revision: Option<u64>,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: String,
    pub range: Option<CommentRange>,
    pub line_text: Option<String>,
    pub body: String,
    /// The thread-resolution decision this comment carries when published
    /// (`Some(true/false)` = resolve/reopen, `None` = no decision). Applied
    /// to the comment's thread; an empty `body` carries only this. `None` on
    /// entries that predate drafted resolution (docs/api.md).
    #[serde(default)]
    pub resolved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyPayload {
    pub replies: Vec<ReplyItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyItem {
    pub id: u64,
    pub comment_id: u64,
    pub body: String,
    /// Thread-resolution decision: `Some(true)` resolves, `Some(false)`
    /// reopens, `None` leaves the thread unchanged.
    #[serde(default)]
    pub resolved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialPayload {
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainClosedPayload {
    pub status: String, // merged | abandoned
}

// ---------------------------------------------------------------------------
// A parsed log entry

#[derive(Debug, Clone)]
pub struct Entry {
    pub idx: u64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

impl Entry {
    /// # Errors
    /// When the stored payload is not valid JSON.
    pub fn from_row(row: &db::LogRow) -> Result<Entry> {
        Ok(Entry {
            idx: row.idx,
            kind: row.kind.clone(),
            payload: serde_json::from_str(&row.payload)
                .map_err(|e| anyhow!("log entry {}: bad payload: {e}", row.idx))?,
            created_at: row.created_at.clone(),
        })
    }

    fn parse<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        serde_json::from_value(self.payload.clone())
            .map_err(|e| anyhow!("log entry {}: {} payload: {e}", self.idx, self.kind))
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

#[derive(Debug, Clone)]
pub struct CommentProj {
    pub id: u64,
    pub change_id: u64,
    pub revision: u64,
    pub parent_id: Option<u64>,
    pub author: String,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub side: String,
    pub range: Option<CommentRange>,
    pub line_text: Option<String>,
    pub body: String,
    pub resolved: bool,
    pub review_id: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ReviewProj {
    pub id: u64,
    pub revision: u64,
    pub verdict: String,
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
    pub comments: Vec<CommentProj>,
    pub reviews: Vec<ReviewProj>,
}

impl ChangeProj {
    #[must_use]
    pub fn latest_revision(&self) -> Option<&RevisionProj> {
        self.revisions.last()
    }

    /// The wire status string: "orphaned" while orphaned, else the
    /// retained status.
    #[must_use]
    pub fn status_str(&self) -> &'static str {
        if self.orphaned {
            "orphaned"
        } else {
            self.status.as_str()
        }
    }

    #[must_use]
    pub fn revision(&self, number: u64) -> Option<&RevisionProj> {
        self.revisions.iter().find(|r| r.number == number)
    }

    /// Count of unresolved root threads (published roots not yet resolved).
    #[must_use]
    pub fn unresolved_roots(&self) -> usize {
        self.comments
            .iter()
            .filter(|c| c.parent_id.is_none() && !c.resolved)
            .count()
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
    pub repo_path: String,
    pub branch: String,
    pub base: String,
    pub created_at: String,
    pub status: ChainStatus,
    pub partial: bool,
    pub changes: Vec<ChangeProj>,
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
            repo_path: chain.repo_path.clone(),
            branch: chain.branch.clone(),
            base: chain.base.clone(),
            created_at: chain.created_at.clone(),
            status: ChainStatus::Active,
            partial: false,
            changes: Vec::new(),
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

    /// A published comment by id (root or reply).
    #[must_use]
    pub fn comment_by_id(&self, id: u64) -> Option<&CommentProj> {
        self.changes
            .iter()
            .find_map(|c| c.comments.iter().find(|cm| cm.id == id))
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

    /// The root comment of a published comment id (walks a reply up to its
    /// root). The owning change is reachable via `CommentProj::change_id`.
    #[must_use]
    pub fn root_comment(&self, comment_id: u64) -> Option<&CommentProj> {
        for change in &self.changes {
            if let Some(c) = change.comments.iter().find(|c| c.id == comment_id) {
                return Some(match c.parent_id {
                    None => c,
                    Some(pid) => change.comments.iter().find(|r| r.id == pid).unwrap_or(c),
                });
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Fold

/// Apply one entry to the projection (docs/data-model.md "The fold").
///
/// # Errors
/// When a payload fails to parse.
pub fn fold(proj: &mut Projection, entry: &Entry) -> Result<()> {
    match entry.kind.as_str() {
        "revisions" => fold_revisions(proj, &entry.parse()?, &entry.created_at),
        "review" => fold_review(proj, &entry.parse()?, &entry.created_at),
        "reply" => fold_reply(proj, &entry.parse()?, &entry.created_at),
        "partial" => proj.partial = entry.parse::<PartialPayload>()?.partial,
        "chain_closed" => {
            proj.status = match entry.parse::<ChainClosedPayload>()?.status.as_str() {
                "abandoned" => ChainStatus::Abandoned,
                _ => ChainStatus::Merged,
            };
        }
        other => return Err(anyhow!("log entry {}: unknown kind {other:?}", entry.idx)),
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
                comments: Vec::new(),
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
    let change_id = change.id;
    change.reviews.push(ReviewProj {
        id: p.review_id,
        revision: p.revision,
        verdict: p.verdict.clone(),
        message: p.message.clone(),
        created_at: now.to_string(),
    });
    for c in &p.comments {
        // An empty-body draft carries only a staged resolution: it updates
        // its thread without materializing as a comment (docs/data-model.md).
        if !c.body.trim().is_empty() {
            change.comments.push(CommentProj {
                id: c.id,
                change_id,
                // A comment stays pinned to the revision it was authored on
                // (a draft on an older patchset / interdiff old side), not the
                // review's target.
                revision: c.revision.unwrap_or(p.revision),
                parent_id: c.parent_id,
                author: "reviewer".to_string(),
                file: c.file.clone(),
                line: c.line,
                side: c.side.clone(),
                range: c.range,
                line_text: c.line_text.clone(),
                body: c.body.clone(),
                resolved: false,
                review_id: Some(p.review_id),
                created_at: now.to_string(),
                updated_at: now.to_string(),
            });
        }
        // Apply the comment's staged resolution to its thread's root, in
        // payload order — the thread ends at the last decision.
        if let Some(state) = c.resolved {
            set_thread_resolved(change, c.parent_id.unwrap_or(c.id), state, now);
        }
    }
    change.status = Status::from_verdict(&p.verdict);
}

/// Set a thread's resolution by its root id (a no-op if the root is absent).
fn set_thread_resolved(change: &mut ChangeProj, root_id: u64, resolved: bool, now: &str) {
    if let Some(root) = change.comments.iter_mut().find(|c| c.id == root_id) {
        root.resolved = resolved;
        root.updated_at = now.to_string();
    }
}

fn fold_reply(proj: &mut Projection, p: &ReplyPayload, now: &str) {
    for r in &p.replies {
        // Resolve the root comment's change + anchor before mutating.
        let Some((change_idx, root)) = locate_root(proj, r.comment_id) else {
            continue;
        };
        let change_id = proj.changes[change_idx].id;
        let new = CommentProj {
            id: r.id,
            change_id,
            revision: root.revision,
            parent_id: Some(root.id),
            author: "agent".to_string(),
            file: root.file.clone(),
            line: root.line,
            side: root.side.clone(),
            range: root.range,
            line_text: root.line_text.clone(),
            body: r.body.clone(),
            resolved: false,
            review_id: None,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        };
        let root_id = root.id;
        let change = &mut proj.changes[change_idx];
        change.comments.push(new);
        if let Some(state) = r.resolved {
            set_thread_resolved(change, root_id, state, now);
        }
    }
}

/// Locate `(change index, root comment clone-free ref)` for a comment id,
/// walking a reply up to its root.
fn locate_root(proj: &Projection, comment_id: u64) -> Option<(usize, &CommentProj)> {
    for (i, change) in proj.changes.iter().enumerate() {
        if let Some(c) = change.comments.iter().find(|c| c.id == comment_id) {
            let root = match c.parent_id {
                None => c,
                Some(pid) => change.comments.iter().find(|r| r.id == pid).unwrap_or(c),
            };
            return Some((i, root));
        }
    }
    None
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
        match entry.kind.as_str() {
            "revisions" => {
                for l in entry.parse::<RevisionsPayload>()?.live {
                    max = max.max(l.change_id);
                }
            }
            "review" => {
                let p: ReviewPayload = entry.parse()?;
                max = max.max(p.review_id);
                for c in p.comments {
                    max = max.max(c.id);
                }
            }
            "reply" => {
                for r in entry.parse::<ReplyPayload>()?.replies {
                    max = max.max(r.id);
                }
            }
            _ => {}
        }
    }
    Ok(max)
}

// ---------------------------------------------------------------------------
// Derived chain state + wake rule

/// Derived chain state (docs/data-model.md "Derived chain state").
#[must_use]
pub fn derive_state(proj: &Projection) -> &'static str {
    match proj.status {
        ChainStatus::Merged => "merged",
        ChainStatus::Abandoned => "abandoned",
        ChainStatus::Active => {
            let live: Vec<&ChangeProj> = proj.changes.iter().filter(|c| !c.orphaned).collect();
            if live.is_empty() {
                return "agents_turn"; // empty chain
            }
            if live
                .iter()
                .any(|c| matches!(c.status, Status::ChangesRequested | Status::Commented))
            {
                "agents_turn"
            } else if live.iter().any(|c| c.status != Status::Approved) {
                "waiting_for_review"
            } else if proj.partial {
                "agents_turn"
            } else {
                "approved"
            }
        }
    }
}

#[must_use]
pub fn actionable(state: &str) -> bool {
    state != "waiting_for_review"
}

#[cfg(test)]
mod tests;
