//! A change, and what folding its log says about it.

use serde::{Deserialize, Serialize};

use super::ChangeId;
use super::ChangeNumber;
use super::ChangeStatus;
use super::CommentInput;
use super::RevisionNumber;
use super::Sha;
use super::ThreadProjection;
use super::Verdict;

/// A change's terminal lifecycle, folded from its `lifecycle` entries.
///
/// The merged commit's sha stays on the `merged` log entry, not here —
/// the fold answers "is it merged", the log answers "as what".
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
pub struct RevisionProjection {
    /// 0-based, minted in the fold.
    pub number: RevisionNumber,
    pub commit_sha: Sha,
    pub parent_sha: Sha,
    pub fork_sha: Sha,
    pub message: String,
    /// `false` for a pure rebase — the revision inherits the prior status.
    pub resets_status: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ReviewProjection {
    /// The `position` of the `review` entry this is the fold of.
    ///
    /// A log coordinate, reproduced by replay with nothing stored.
    pub id: u64,
    pub revision: RevisionNumber,
    pub verdict: Verdict,
    pub message: String,
    pub created_at: String,
}

/// The fold of one change's log.
///
/// Serializable so a fold can be handed on and resumed against the live
/// tail of the log instead of replayed from the start. The wire form is
/// opaque: a projection is only ever produced and consumed by the fold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeProjection {
    pub id: ChangeNumber,
    pub repo_id: u64,
    pub change_id: ChangeId,
    pub revisions: Vec<RevisionProjection>,
    pub threads: Vec<ThreadProjection>,
    pub reviews: Vec<ReviewProjection>,
    pub lifecycle: Lifecycle,
    /// Bumped each time a thread is opened.
    pub next_thread_id: u64,
    /// Count of entries folded = the next unconsumed `position`.
    ///
    /// A high-water mark, carried in the projection so a resumed fold
    /// starts at the right boundary and stays idempotent across the
    /// overlap.
    pub entries_folded: u64,
}

/// A commit message's subject: its leading paragraph, folded to one line.
///
/// # Examples
///
/// ```rust
/// use nit_types::domain::subject_of;
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

impl ChangeProjection {
    /// The fold builds the rest from the log.
    #[must_use]
    pub fn new(id: ChangeNumber, repo_id: u64, change_id: ChangeId) -> ChangeProjection {
        ChangeProjection {
            id,
            repo_id,
            change_id,
            revisions: Vec::new(),
            threads: Vec::new(),
            reviews: Vec::new(),
            lifecycle: Lifecycle::Active,
            next_thread_id: 0,
            entries_folded: 0,
        }
    }

    #[must_use]
    pub fn latest_revision(&self) -> Option<&RevisionProjection> {
        self.revisions.last()
    }

    #[must_use]
    pub fn revision(&self, number: RevisionNumber) -> Option<&RevisionProjection> {
        self.revisions.iter().find(|r| r.number == number)
    }

    #[must_use]
    pub fn thread(&self, id: u64) -> Option<&ThreadProjection> {
        self.threads.iter().find(|t| t.id == id)
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        !matches!(self.lifecycle, Lifecycle::Active)
    }

    /// Whether the change has **merged** onto the canonical ref.
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
    pub fn subject_at(&self, revision: RevisionNumber) -> String {
        self.revision(revision)
            .map(|r| subject_of(&r.message))
            .unwrap_or_default()
    }

    /// The change's current status.
    ///
    /// [`status_at`](Self::status_at) its latest revision (pending when it
    /// has none).
    #[must_use]
    pub fn current_status(&self) -> ChangeStatus {
        self.status_at(
            self.latest_revision()
                .map_or(RevisionNumber::new(0), |r| r.number),
        )
    }

    /// The displayed status at a pinned revision.
    ///
    /// The lifecycle overlay (`abandoned` change-wide, `merged` at the
    /// latest revision) over the verdict-derived review status
    /// (`review_status_at`).
    #[must_use]
    pub fn status_at(&self, revision: RevisionNumber) -> ChangeStatus {
        if matches!(self.lifecycle, Lifecycle::Abandoned) {
            return ChangeStatus::Abandoned;
        }
        // A merge may land content matching no recorded revision — it is
        // recorded against the change, not a revision — so the latest
        // stands in for it.
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
    fn review_status_at(&self, revision: RevisionNumber) -> ChangeStatus {
        if let Some(rv) = self
            .reviews
            .iter()
            .filter(|r| r.revision == revision)
            .max_by_key(|r| r.id)
        {
            return rv.verdict.into();
        }
        // No review here: a pure-rebase revision carries the prior one forward.
        if let Some(previous) = revision.previous()
            && self.revision(revision).is_some_and(|r| !r.resets_status)
        {
            return self.review_status_at(previous);
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
