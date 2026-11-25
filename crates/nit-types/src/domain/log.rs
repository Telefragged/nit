//! The log: one entry per thing that happened to a change.

use serde::{Deserialize, Serialize};

use super::AttachPayload;
use super::ChangeNumber;
use super::CommentRange;
use super::FileAnchor;
use super::LogAnchor;
use super::RevisionNumber;
use super::Sha;
use super::Side;
use super::Tags;
use super::Verdict;

/// The kind of one log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Revision,
    Review,
    Comment,
    Lifecycle,
    Tags,
    Attach,
}

impl LogKind {
    /// The wire spelling (mirrors the serde renaming).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LogKind::Revision => "revision",
            LogKind::Review => "review",
            LogKind::Comment => "comment",
            LogKind::Lifecycle => "lifecycle",
            LogKind::Tags => "tags",
            LogKind::Attach => "attach",
        }
    }
}

impl std::str::FromStr for LogKind {
    type Err = String;

    fn from_str(s: &str) -> Result<LogKind, String> {
        match s {
            "revision" => Ok(LogKind::Revision),
            "review" => Ok(LogKind::Review),
            "comment" => Ok(LogKind::Comment),
            "lifecycle" => Ok(LogKind::Lifecycle),
            "tags" => Ok(LogKind::Tags),
            "attach" => Ok(LogKind::Attach),
            other => Err(format!("unknown log entry kind {other:?}")),
        }
    }
}

/// What a `lifecycle` log entry records about a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    Merged,
    Abandoned,
    Reopened,
}

impl LifecycleAction {
    /// The wire spelling (mirrors the serde renaming).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleAction::Merged => "merged",
            LifecycleAction::Abandoned => "abandoned",
            LifecycleAction::Reopened => "reopened",
        }
    }
}

/// A `revision` entry: one new commit-sha observed for this change.
///
/// The payload contains no revision `number`. The fold mints it, so a
/// concurrent push to the same change cannot duplicate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct RevisionPayload {
    pub commit_sha: Sha,
    pub parent_sha: Sha,
    pub fork_sha: Sha,
    pub message: String,
    /// `false` only for a pure rebase: the message is unchanged, and the
    /// tree equals the prior commit replayed onto the new parent.
    ///
    /// The new revision then inherits the prior revision's review status
    /// rather than resetting to `pending`.
    pub resets_status: bool,
}

/// A `tags` entry: the labels it puts on a change.
///
/// The fold lays it over the change's current set, so a key this entry
/// omits keeps the value it had and a key it sets takes a new one. The
/// entry stands on its own, so labelling a change costs no revision and
/// disturbs no review status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct TagsPayload {
    pub tags: Tags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct ReviewPayload {
    pub revision: RevisionNumber,
    pub verdict: Verdict,
    pub message: String,
    /// The drained drafts, in draft order.
    ///
    /// Each opens a new thread or replies to an existing one (see
    /// [`CommentInput`]).
    pub comments: Vec<CommentInput>,
}

/// A comment inside a `review` or `comment` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(try_from = "LoggedComment")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommentInput {
    #[serde(flatten)]
    target: CommentTarget,
    body: String,
    /// Thread-resolution decision.
    ///
    /// `Some(true/false)` = resolve/reopen, `None` = no decision. On a new
    /// thread it is the birth state; a reply with an empty `body` contains
    /// only this.
    #[serde(default)]
    resolved: Option<bool>,
}

/// The thread that a comment opens or replies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(untagged)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum CommentTarget {
    /// Opens a new thread at `anchor` of `revision`.
    ///
    /// `thread_id` is the id of the new thread. The append that records
    /// the comment assigns it, so a draft has none.
    Open {
        thread_id: Option<u64>,
        revision: RevisionNumber,
        anchor: LogAnchor,
    },
    /// Replies to thread `thread_id`, at the anchor of that thread.
    Reply { thread_id: u64 },
}

impl CommentInput {
    /// Returns the comment that opens a thread at `anchor` of `revision`.
    ///
    /// # Errors
    ///
    /// [`CommentInputError::EmptyOpening`] when `body` is blank.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::{CommentInput, LogAnchor, RevisionNumber};
    ///
    /// let r0 = RevisionNumber::new(0);
    /// assert!(CommentInput::open(r0, LogAnchor::Change, "why?".into(), None).is_ok());
    /// assert!(CommentInput::open(r0, LogAnchor::Change, " ".into(), None).is_err());
    /// ```
    pub fn open(
        revision: RevisionNumber,
        anchor: LogAnchor,
        body: String,
        resolved: Option<bool>,
    ) -> Result<CommentInput, CommentInputError> {
        if body.trim().is_empty() {
            return Err(CommentInputError::EmptyOpening);
        }
        Ok(CommentInput {
            target: CommentTarget::Open {
                thread_id: None,
                revision,
                anchor,
            },
            body,
            resolved,
        })
    }

    /// Returns the comment that replies to thread `thread_id`.
    ///
    /// # Errors
    ///
    /// [`CommentInputError::EmptyReply`] when `body` is blank and the reply
    /// sets no resolution.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::CommentInput;
    ///
    /// assert!(CommentInput::reply(0, String::new(), Some(true)).is_ok());
    /// assert!(CommentInput::reply(0, String::new(), None).is_err());
    /// ```
    pub fn reply(
        thread_id: u64,
        body: String,
        resolved: Option<bool>,
    ) -> Result<CommentInput, CommentInputError> {
        if body.trim().is_empty() && resolved.is_none() {
            return Err(CommentInputError::EmptyReply);
        }
        Ok(CommentInput {
            target: CommentTarget::Reply { thread_id },
            body,
            resolved,
        })
    }

    /// Records `id` as the thread that the comment opens.
    ///
    /// A reply, and a comment that opens a thread with an id already, stay
    /// as they are.
    pub fn assign_thread_id(&mut self, id: u64) {
        if let CommentTarget::Open {
            thread_id: thread_id @ None,
            ..
        } = &mut self.target
        {
            *thread_id = Some(id);
        }
    }

    #[must_use]
    pub fn target(&self) -> &CommentTarget {
        &self.target
    }

    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    #[must_use]
    pub fn resolved(&self) -> Option<bool> {
        self.resolved
    }

    #[must_use]
    pub fn thread_id(&self) -> Option<u64> {
        match self.target {
            CommentTarget::Open { thread_id, .. } => thread_id,
            CommentTarget::Reply { thread_id } => Some(thread_id),
        }
    }
}

/// The log is append-only, so entries written before a comment had one
/// [`LogAnchor`] keep the five loose fields it was spelled with.
#[derive(Deserialize)]
struct LoggedComment {
    #[serde(default)]
    thread_id: Option<u64>,
    #[serde(default)]
    revision: Option<RevisionNumber>,
    #[serde(default)]
    anchor: Option<LogAnchor>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<u64>,
    /// Set on an opening comment and unset on a reply, which is how the
    /// older spelling marked which of the two an entry is.
    #[serde(default)]
    side: Option<Side>,
    #[serde(default)]
    range: Option<CommentRange>,
    body: String,
    #[serde(default)]
    resolved: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommentInputError {
    #[error("a comment that opens a thread records the revision of its anchor")]
    OpeningWithoutRevision,
    #[error("a comment opens a thread at an anchor or replies to a thread")]
    NoTarget,
    #[error("a new thread requires a body")]
    EmptyOpening,
    #[error("a reply requires a body or a resolution decision")]
    EmptyReply,
}

impl TryFrom<LoggedComment> for CommentInput {
    type Error = CommentInputError;

    fn try_from(c: LoggedComment) -> Result<CommentInput, CommentInputError> {
        let anchor = c.anchor.or_else(|| {
            let side = c.side?;
            Some(match c.file {
                Some(path) => LogAnchor::File {
                    path,
                    side,
                    anchor: FileAnchor::from_parts(c.range, c.line),
                },
                None => LogAnchor::Change,
            })
        });
        match (anchor, c.thread_id) {
            (Some(anchor), thread_id) => {
                let revision = c
                    .revision
                    .ok_or(CommentInputError::OpeningWithoutRevision)?;
                let mut comment = CommentInput::open(revision, anchor, c.body, c.resolved)?;
                if let Some(id) = thread_id {
                    comment.assign_thread_id(id);
                }
                Ok(comment)
            }
            (None, Some(thread_id)) => CommentInput::reply(thread_id, c.body, c.resolved),
            (None, None) => Err(CommentInputError::NoTarget),
        }
    }
}

/// A `lifecycle` entry: a merge, an abandon, or a reopen.
///
/// `commit_sha` is set only for `merged` — the merged commit on the
/// canonical ref; `message` is an optional reason on `abandoned`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct LifecyclePayload {
    pub action: LifecycleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<Sha>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A log entry's payload as a closed union tagged by `kind`.
///
/// Flattened into [`LogEntry`], the adjacent tag produces the wire's
/// `{…, "kind": …, "payload": …}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum LogPayload {
    Revision(RevisionPayload),
    Review(ReviewPayload),
    /// One author comment (the `comment` kind), opening a thread or replying.
    Comment(CommentInput),
    Lifecycle(LifecyclePayload),
    Tags(TagsPayload),
    Attach(AttachPayload),
}

impl LogPayload {
    /// The kind tag this entry stores under.
    #[must_use]
    pub fn kind(&self) -> LogKind {
        match self {
            LogPayload::Revision(_) => LogKind::Revision,
            LogPayload::Review(_) => LogKind::Review,
            LogPayload::Comment(_) => LogKind::Comment,
            LogPayload::Lifecycle(_) => LogKind::Lifecycle,
            LogPayload::Tags(_) => LogKind::Tags,
            LogPayload::Attach(_) => LogKind::Attach,
        }
    }

    /// A `lifecycle` entry from its parts.
    #[must_use]
    pub fn lifecycle(
        action: LifecycleAction,
        commit_sha: Option<Sha>,
        message: Option<String>,
    ) -> LogPayload {
        LogPayload::Lifecycle(LifecyclePayload {
            action,
            commit_sha,
            message,
        })
    }
}

/// One log entry.
///
/// Belongs to one change, and `position` orders the entries of the change.
/// The flattened [`LogPayload`] contributes the `kind` discriminant and the
/// `payload` body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct LogEntry {
    pub change_number: ChangeNumber,
    pub position: u64,
    pub created_at: String,
    #[serde(flatten)]
    pub payload: LogPayload,
}
