//! The fold: a **change's** reviewable state is the replay of its log.
//!
//! The log is append-only. [`ChangeProj`] is the in-memory state machine;
//! [`fold`] applies one wire [`LogEntry`]; [`replay`] rebuilds a change's
//! projection from its entries. A chain is never folded — it is composed
//! at read time from member projections (`crate::chain`).
//!
//! Pure over `nit_types` alone: no database, no storage serialization, no event
//! publishing. The server's db/storage adapters (`crate::review`) feed it wire
//! `LogEntry`s and store/broadcast the entries it returns; the same code folds
//! the websocket stream client-side once compiled to WebAssembly.
//!
//! Fold-assigned ids: a review's id is its `review` entry's `idx` — a log
//! coordinate, reproduced by replay with nothing stored and nothing minted.
//! The change id is the `changes` rowid, carried on the projection.
//! Revision numbers (0-based) are minted **in the fold** by creation order — a
//! pure function of the log, never stored. Thread ids are minted in the fold
//! too: [`fold`] takes an entry by value and, via
//! [`ChangeProj::mint_thread_id`], fills a new-thread comment's `thread_id` from
//! `next_thread_id` and returns the entry with the id written into its payload,
//! so the caller stores and broadcasts that one value. `next_thread_id` is the
//! single source of truth — the only field minting touches — so a concurrent
//! shared-change push can't duplicate an id, and replay (ids already set) just
//! advances it. The fold therefore requires entries in ascending `idx` order.
//!
//! [`ChangeProj::entries_folded`] is the count of entries consumed (the next
//! `idx`): the server stamps it into a snapshot so a follower resumes folding
//! the live tail at the boundary, and [`fold`] skips any entry below it, so the
//! arm/snapshot overlap is idempotent, never doubled.

use serde::{Deserialize, Serialize};

use crate::changes::{ChangeDetail, Review, Revision};
use crate::comments::{CommentRange, Thread};
use crate::domain::{ChangeStatus, LifecycleAction, Side, Verdict};
use crate::log::{CommentInput, LifecyclePayload, LogEntry, LogPayload, RevisionPayload};

/// A change's terminal lifecycle, folded from its `lifecycle` entries.
///
/// The landed commit's sha stays on the `merged` log entry, not here —
/// the fold answers "is it landed", the log answers "as what".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Active,
    Merged,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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

/// Where a thread is anchored within a revision.
///
/// Modeled so the invalid combinations the flat wire fields allow are
/// unrepresentable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
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

/// A located, resolvable conversation.
///
/// Its anchor and birth come from its first comment; the `id` is
/// fold-assigned by creation order, never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
///
/// `review_id` is the review that published it, or `None` for an agent's
/// own note — which is what distinguishes reviewer from agent (the only
/// consumer derives the label from it).
#[derive(Debug, Clone, Serialize, Deserialize)]
// Shares the wire `ThreadComment` name but is a distinct type — only
// ever round-tripped through the wasm fold.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ThreadCommentProj"))]
pub struct ThreadComment {
    pub body: String,
    pub review_id: Option<u64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ReviewProj {
    /// The `idx` of the `review` entry this is the fold of.
    ///
    /// A log coordinate, reproduced by replay with nothing stored.
    pub id: u64,
    pub revision: u64,
    pub verdict: Verdict,
    pub message: String,
    pub created_at: String,
}

/// The fold of one change's log.
///
/// Serializable so the server can ship it as the subscribe **snapshot**
/// and the browser can resume folding the live tail from it; the wire
/// form is opaque to the web, which only passes it back through the
/// shared WebAssembly fold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeProj {
    pub id: u64,
    pub repo_id: u64,
    pub change_key: String,
    pub revisions: Vec<RevisionProj>,
    pub threads: Vec<ThreadProj>,
    pub reviews: Vec<ReviewProj>,
    pub lifecycle: Lifecycle,
    /// Bumped each time a thread is opened.
    pub next_thread_id: u64,
    /// Count of entries folded = the next unconsumed `idx`.
    ///
    /// A high-water mark, carried in the snapshot so the client resumes
    /// folding the live tail at the right boundary and [`fold`] stays
    /// idempotent across the overlap.
    pub entries_folded: u64,
}

/// Commit subject, matching git's `find_commit_subject` + `format_subject`.
///
/// # Examples
///
/// ```rust
/// use nit_types::fold::subject_of;
///
/// assert_eq!(subject_of("one line\n\nbody"), "one line");
/// assert_eq!(subject_of("wrapped\nsubject\n\nbody"), "wrapped subject");
/// assert_eq!(subject_of("\n\nleading blank"), "leading blank");
/// assert_eq!(subject_of("trailing newline\n"), "trailing newline");
/// assert_eq!(subject_of(""), "");
/// ```
#[must_use]
pub fn subject_of(message: &str) -> String {
    let body = message.trim_start_matches(['\n', '\r']);
    let para = body.split("\n\n").next().unwrap_or("");
    para.replace('\n', " ").trim().to_string()
}

impl ChangeProj {
    /// The fold builds the rest from the log.
    #[must_use]
    pub fn new(id: u64, repo_id: u64, change_key: String) -> ChangeProj {
        ChangeProj {
            id,
            repo_id,
            change_key,
            revisions: Vec::new(),
            threads: Vec::new(),
            reviews: Vec::new(),
            lifecycle: Lifecycle::Active,
            next_thread_id: 0,
            entries_folded: 0,
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

    /// Whether the change has **landed** on the canonical branch.
    ///
    /// Distinct from `is_terminal`: an abandoned change is terminal but
    /// not merged, and stays an enumerable member/tip of its chains
    /// (abandonment is membership-inert).
    #[must_use]
    pub fn is_merged(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Merged)
    }

    /// Returns one revision's commit-message subject.
    ///
    /// Empty when the revision is unknown.
    #[must_use]
    pub fn subject_at(&self, revision: u64) -> String {
        self.revision(revision)
            .map(|r| subject_of(&r.message))
            .unwrap_or_default()
    }

    /// The change's current status.
    ///
    /// [`status_at`](Self::status_at) its latest revision (pending when it
    /// has none). The denormalized `changes.status` column
    /// (`crates/nit/src/db.rs`) caches this so a query can filter changes
    /// without folding their logs.
    #[must_use]
    pub fn current_status(&self) -> ChangeStatus {
        self.status_at(self.latest_revision().map_or(0, |r| r.number))
    }

    /// The displayed status at a pinned revision.
    ///
    /// The lifecycle overlay (`abandoned` change-wide, `merged` at the
    /// latest revision) over the verdict-derived review status
    /// (`review_status_at`).
    #[must_use]
    pub fn status_at(&self, revision: u64) -> ChangeStatus {
        if matches!(self.lifecycle, Lifecycle::Abandoned) {
            return ChangeStatus::Abandoned;
        }
        // A landing may carry content matching no recorded revision (the
        // approve action rebases before merging; the timer only records the
        // landing by Change-Id), so the latest stands in for it.
        if self.is_merged() && self.latest_revision().is_some_and(|r| r.number == revision) {
            return ChangeStatus::Merged;
        }
        self.review_status_at(revision)
    }

    /// The verdict-derived status at a revision.
    ///
    /// The latest review on it, else the prior revision's status when this
    /// one is a pure rebase, else pending. Never the lifecycle-overlay
    /// values (`merged`/`abandoned`).
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

    /// Resolves a comment's thread id and keeps `next_thread_id` past it.
    ///
    /// `next_thread_id` is the single source of truth. Called before each
    /// fold: a live append mints (the stored payload then carries the id)
    /// while replay, seeing the id already set, only advances the
    /// counter — no double count.
    pub fn mint_thread_id(&mut self, comment: &mut CommentInput) {
        if comment.thread_id.is_none() && !comment.body.trim().is_empty() {
            comment.thread_id = Some(self.next_thread_id);
        }
        if let Some(id) = comment.thread_id {
            self.next_thread_id = self.next_thread_id.max(id + 1);
        }
    }
}

/// Applies one wire entry to a change's projection.
///
/// Mints any new-thread ids into the entry's typed payload and returns
/// the id-bearing entry (the server stores and broadcasts that one).
pub fn fold(change: &mut ChangeProj, mut entry: LogEntry) -> LogEntry {
    // Idempotent across the snapshot/live overlap: an entry already folded into
    // this projection (its idx below the high-water mark) leaves it untouched,
    // so a follower that re-receives the boundary entries the snapshot already
    // covers never double-applies them.
    if entry.idx < change.entries_folded {
        return entry;
    }
    change.entries_folded = entry.idx + 1;
    let now = entry.created_at.clone();
    // A review is identified by where it sits in its change's log: replay
    // reproduces the id with nothing stored and nothing minted, and no reader
    // outside this fold references a review at all.
    let review_id = entry.idx;
    match &mut entry.payload {
        LogPayload::Revision(p) => fold_revision(change, p, &now),
        LogPayload::Review(p) => {
            change.reviews.push(ReviewProj {
                id: review_id,
                revision: p.revision,
                verdict: p.verdict,
                message: p.message.clone(),
                created_at: now.clone(),
            });
            for c in &mut p.comments {
                change.mint_thread_id(c);
                apply_comment(change, c, Some(review_id), &now);
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
        LifecycleAction::Merged => Lifecycle::Merged,
        LifecycleAction::Abandoned => Lifecycle::Abandoned,
        LifecycleAction::Reopened => Lifecycle::Active,
    };
}

/// Applies one comment to a change's threads.
///
/// Its `thread_id` is already resolved by [`ChangeProj::mint_thread_id`];
/// shared by `review` and `comment`. An unset id is a no-op: the mint left
/// it alone because the body was empty.
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

/// Opens a new thread carrying `id` at the comment's anchor.
///
/// `next_thread_id` is kept ahead by [`ChangeProj::mint_thread_id`], the
/// sole owner of the counter.
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

/// Rebuilds a change's projection from `entries`.
///
/// Requires ascending `idx` — `fold()`'s high-water mark silently skips
/// anything out of order.
#[must_use]
pub fn replay(id: u64, repo_id: u64, change_key: String, entries: Vec<LogEntry>) -> ChangeProj {
    let mut change = ChangeProj::new(id, repo_id, change_key);
    for entry in entries {
        fold(&mut change, entry);
    }
    change
}

// Projection → wire: the published view of a change, shared by the
// server's change endpoint and the WebAssembly fold.

#[must_use]
pub fn revision_view(rev: &RevisionProj) -> Revision {
    Revision {
        number: rev.number,
        commit_sha: rev.commit_sha.clone(),
        parent_sha: rev.parent_sha.clone(),
        base_sha: rev.base_sha.clone(),
        message: rev.message.clone(),
        created_at: rev.created_at.clone(),
    }
}

#[must_use]
pub fn review_view(review: &ReviewProj) -> Review {
    Review {
        id: review.id,
        revision: review.revision,
        verdict: review.verdict,
        message: review.message.clone(),
        created_at: review.created_at.clone(),
    }
}

#[must_use]
pub fn thread_view(t: &ThreadProj, change_id: u64) -> Thread {
    let (file, line, side, range, line_text) = match &t.anchor {
        Anchor::Change => (None, None, Side::New, None, None),
        Anchor::File { file } => (Some(file.clone()), None, Side::New, None, None),
        Anchor::Line {
            file,
            side,
            line,
            line_text,
            range,
        } => (
            Some(file.clone()),
            Some(*line),
            *side,
            *range,
            line_text.clone(),
        ),
    };
    Thread {
        id: t.id,
        change_id,
        revision: t.revision,
        file,
        line,
        side,
        range,
        line_text,
        resolved: t.resolved,
        comments: t.comments.iter().map(thread_comment_view).collect(),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
    }
}

fn thread_comment_view(c: &ThreadComment) -> crate::comments::ThreadComment {
    crate::comments::ThreadComment {
        body: c.body.clone(),
        review_id: c.review_id,
        created_at: c.created_at.clone(),
    }
}

/// The published projection of a change as the wire [`ChangeDetail`].
///
/// Minus the reviewer's drafts and staged decision: mutable scratch
/// outside the log that the server overlays from the database. The
/// WebAssembly fold returns this verbatim and the browser fills its own
/// drafts in.
#[must_use]
pub fn change_detail(change: &ChangeProj) -> ChangeDetail {
    ChangeDetail {
        id: change.id,
        repo_id: change.repo_id,
        change_key: change.change_key.clone(),
        revisions: change.revisions.iter().map(revision_view).collect(),
        threads: change
            .threads
            .iter()
            .map(|t| thread_view(t, change.id))
            .collect(),
        drafts: Vec::new(),
        reviews: change.reviews.iter().map(review_view).collect(),
        draft_decision: None,
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{ChangeStatus, LifecycleAction, Side, Verdict};
    use crate::log::ReviewPayload;

    use super::*;

    fn empty() -> ChangeProj {
        ChangeProj::new(1, 1, "Iabc".to_string())
    }

    fn entry(idx: u64, payload: LogPayload) -> LogEntry {
        LogEntry {
            change_id: 1,
            seq: idx,
            idx,
            created_at: format!("t{idx}"),
            payload,
        }
    }

    /// A `revision` payload; the fold mints its 0-based number.
    fn revision(sha: &str, parent: &str, base: &str, resets: bool) -> LogPayload {
        LogPayload::Revision(RevisionPayload {
            commit_sha: sha.to_string(),
            parent_sha: parent.to_string(),
            base_sha: base.to_string(),
            message: format!("subject {sha}\n\nChange-Id: Iabc\n"),
            resets_status: resets,
        })
    }

    fn review(revision: u64, verdict: Verdict) -> LogPayload {
        LogPayload::Review(ReviewPayload {
            revision,
            verdict,
            message: "msg".to_string(),
            comments: vec![],
        })
    }

    fn anchored(file: &str, line: u64, body: &str) -> CommentInput {
        CommentInput {
            thread_id: None,
            revision: Some(0),
            file: Some(file.to_string()),
            line: Some(line),
            side: Some(Side::New),
            range: None,
            line_text: None,
            body: body.to_string(),
            resolved: None,
        }
    }

    fn cinput(thread_id: Option<u64>, body: &str) -> CommentInput {
        CommentInput {
            thread_id,
            revision: None,
            file: None,
            line: None,
            side: None,
            range: None,
            line_text: None,
            body: body.to_string(),
            resolved: None,
        }
    }

    fn folded(payloads: Vec<LogPayload>) -> ChangeProj {
        let mut c = empty();
        for (i, payload) in payloads.into_iter().enumerate() {
            fold(
                &mut c,
                entry(u64::try_from(i).expect("index fits u64"), payload),
            );
        }
        c
    }

    #[test]
    fn revisions_are_zero_based_and_minted_in_the_fold() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            revision("B", "A", "base", true),
        ]);
        assert_eq!(c.revisions.len(), 2);
        assert_eq!(c.revisions[0].number, 0);
        assert_eq!(c.revisions[1].number, 1);
        assert_eq!(c.latest_revision().expect("a revision").commit_sha, "B");
    }

    #[test]
    fn status_is_per_revision() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::RequestChanges),
            revision("B", "base", "base", true), // reword: new revision
        ]);
        // The review landed on r0; r1 has no review yet and resets → pending.
        assert_eq!(c.status_at(0), ChangeStatus::ChangesRequested);
        assert_eq!(c.status_at(1), ChangeStatus::Pending);
    }

    #[test]
    fn pure_rebase_carries_status_forward() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            // r1 is a pure rebase (resets_status = false): inherits r0's approve.
            revision("B", "base2", "base2", false),
        ]);
        assert_eq!(c.status_at(0), ChangeStatus::Approved);
        assert_eq!(c.status_at(1), ChangeStatus::Approved);
    }

    #[test]
    fn reword_resets_status() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            revision("B", "base", "base", true),
        ]);
        assert_eq!(c.status_at(1), ChangeStatus::Pending);
    }

    #[test]
    fn current_status_tracks_the_latest_revision() {
        assert_eq!(empty().current_status(), ChangeStatus::Pending);
        // current_status is the displayed status at the latest revision: r1 has no
        // review, so pending — even though r0 was approved.
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            revision("B", "base", "base", true),
        ]);
        assert_eq!(c.status_at(0), ChangeStatus::Approved);
        assert_eq!(c.current_status(), ChangeStatus::Pending);
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
        ]);
        assert_eq!(c.current_status(), ChangeStatus::Approved);
        // The lifecycle overlay wins change-wide: abandoned regardless of revision.
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            LogPayload::lifecycle(LifecycleAction::Abandoned, None, None),
        ]);
        assert_eq!(c.current_status(), ChangeStatus::Abandoned);
    }

    #[test]
    fn merged_paints_at_latest() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::Approve),
            revision("B", "base", "base", true),
            LogPayload::lifecycle(LifecycleAction::Merged, Some("C".to_string()), None),
        ]);
        // Merged shows at the latest revision; older ones keep their own status.
        assert_eq!(c.status_at(1), ChangeStatus::Merged);
        assert_eq!(c.status_at(0), ChangeStatus::Approved);
        assert!(c.is_terminal());
    }

    #[test]
    fn abandon_then_reopen() {
        let mut c = folded(vec![
            revision("A", "base", "base", true),
            review(0, Verdict::RequestChanges),
            LogPayload::lifecycle(LifecycleAction::Abandoned, None, None),
        ]);
        assert_eq!(c.status_at(0), ChangeStatus::Abandoned);
        assert!(c.is_terminal());
        // Reopen restores the retained verdict status.
        fold(
            &mut c,
            entry(
                3,
                LogPayload::lifecycle(LifecycleAction::Reopened, None, None),
            ),
        );
        assert!(!c.is_terminal());
        assert_eq!(c.status_at(0), ChangeStatus::ChangesRequested);
    }

    #[test]
    fn threads_open_reply_and_resolve() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            LogPayload::Review(ReviewPayload {
                revision: 0,
                verdict: Verdict::Comment,
                message: String::new(),
                comments: vec![anchored("src/x.rs", 3, "look")],
            }),
            LogPayload::Review(ReviewPayload {
                revision: 0,
                verdict: Verdict::Approve,
                message: String::new(),
                comments: vec![CommentInput {
                    thread_id: Some(0),
                    resolved: Some(true),
                    ..cinput(None, "fixed")
                }],
            }),
        ]);
        assert_eq!(c.threads.len(), 1);
        assert_eq!(c.threads[0].comments.len(), 2);
        assert!(c.threads[0].resolved);
    }

    #[test]
    fn agent_comment_opens_a_thread() {
        let c = folded(vec![
            revision("A", "base", "base", true),
            LogPayload::Comment(anchored("a.rs", 1, "why?")),
        ]);
        assert_eq!(c.threads.len(), 1);
        // An agent note carries no review_id — that is what marks it agent-authored.
        assert_eq!(c.threads[0].comments[0].review_id, None);
    }

    #[test]
    fn mint_thread_id_assigns_then_keeps_the_counter_ahead() {
        let mut c = empty();
        let mut open = cinput(None, "opens");
        c.mint_thread_id(&mut open);
        assert_eq!(open.thread_id, Some(0));
        assert_eq!(c.next_thread_id, 1);
        let mut reply = cinput(Some(0), "reply");
        c.mint_thread_id(&mut reply);
        assert_eq!(reply.thread_id, Some(0));
        assert_eq!(c.next_thread_id, 1);
        let mut empty_body = cinput(None, "");
        c.mint_thread_id(&mut empty_body);
        assert_eq!(empty_body.thread_id, None);
        assert_eq!(c.next_thread_id, 1);
        // A stamped id past the counter (a replayed open) pulls it forward.
        let mut stamped = cinput(Some(5), "stamped");
        c.mint_thread_id(&mut stamped);
        assert_eq!(c.next_thread_id, 6);
    }

    #[test]
    fn fold_opens_a_thread_for_a_stamped_unseen_id() {
        let mut c = empty();
        fold(&mut c, entry(0, revision("A", "base", "base", true)));
        fold(
            &mut c,
            entry(
                1,
                LogPayload::Comment(CommentInput {
                    thread_id: Some(3),
                    ..anchored("a.rs", 1, "why?")
                }),
            ),
        );
        assert_eq!(c.threads.len(), 1);
        assert_eq!(c.threads[0].id, 3);
        assert_eq!(c.next_thread_id, 4);
        fold(&mut c, entry(2, LogPayload::Comment(cinput(Some(3), "ok"))));
        assert_eq!(c.threads.len(), 1);
        assert_eq!(c.threads[0].comments.len(), 2);
    }

    #[test]
    fn replay_folds_entries_in_order() {
        let c = replay(
            1,
            1,
            "Iabc".to_string(),
            vec![
                entry(0, revision("A", "base", "base", true)),
                entry(1, review(0, Verdict::Approve)),
            ],
        );
        assert_eq!(c.revisions.len(), 1);
        assert_eq!(c.status_at(0), ChangeStatus::Approved);
    }

    #[test]
    fn entries_folded_tracks_the_high_water_mark_and_dedups_the_overlap() {
        let mut c = empty();
        fold(&mut c, entry(0, revision("A", "base", "base", true)));
        fold(&mut c, entry(1, review(0, Verdict::Approve)));
        assert_eq!(c.entries_folded, 2);
        // Re-delivering the snapshot/live boundary (idx 1) is a no-op.
        fold(&mut c, entry(1, review(0, Verdict::RequestChanges)));
        assert_eq!(c.reviews.len(), 1);
        assert_eq!(c.entries_folded, 2);
        assert_eq!(c.status_at(0), ChangeStatus::Approved);
        fold(&mut c, entry(2, review(0, Verdict::RequestChanges)));
        assert_eq!(c.reviews.len(), 2);
        assert_eq!(c.entries_folded, 3);
    }
}
