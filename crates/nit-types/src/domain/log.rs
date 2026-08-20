//! The log: one entry per thing that happened to a change.

use serde::{Deserialize, Serialize};

use super::ChangeNumber;
use super::CommentRange;
use super::RevisionNumber;
use super::Sha;
use super::Side;
use super::Verdict;

/// The kind of one log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Revision,
    Review,
    Comment,
    Lifecycle,
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
            other => Err(format!("unknown log entry kind {other:?}")),
        }
    }
}

/// What a `lifecycle` log entry records about a change.
///
/// The merge/abandon timer writes `merged`/`abandoned`; `nit reopen`
/// writes `reopened`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    Merged,
    Abandoned,
    Reopened,
}

impl LifecycleAction {
    /// The wire spelling (mirrors the serde renaming), for Value-free display.
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
/// The revision `number` is **not** carried — the fold mints it, so a
/// concurrent shared-change push cannot duplicate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RevisionPayload {
    pub commit_sha: Sha,
    pub parent_sha: Sha,
    pub fork_sha: Sha,
    pub message: String,
    /// `false` only for a pure rebase (patch-id-equal, message unchanged).
    ///
    /// The new revision then inherits the prior revision's review status
    /// rather than resetting to `pending`.
    pub resets_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
///
/// With `thread_id` unset it **opens a new thread** anchored by the
/// fields below; with it set it **replies** to that thread (the anchor is
/// ignored — the thread owns it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommentInput {
    /// `None` opens a new thread; `Some` appends to that thread.
    #[serde(default)]
    pub thread_id: Option<u64>,
    /// Anchor revision for a new thread.
    ///
    /// A draft's own revision — an interdiff old side pins to an earlier
    /// revision. Always set on a recorded comment; the fold falls back to
    /// the change's latest only for a malformed payload.
    #[serde(default)]
    pub revision: Option<RevisionNumber>,
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
    /// Thread-resolution decision.
    ///
    /// `Some(true/false)` = resolve/reopen, `None` = no decision. On a new
    /// thread it is the birth state; a `thread_id` reply with an empty
    /// `body` carries only this.
    #[serde(default)]
    pub resolved: Option<bool>,
}

/// A `lifecycle` entry: a merge, an abandon, or a reopen.
///
/// The merge timer (`merged`) and the `nit abandon` / `nit reopen`
/// actions. `commit_sha` is set only for `merged` — the merged commit on
/// the canonical ref; `message` is an optional reason on `abandoned`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum LogPayload {
    Revision(RevisionPayload),
    Review(ReviewPayload),
    /// One author comment (the `comment` kind), opening a thread or replying.
    Comment(CommentInput),
    Lifecycle(LifecyclePayload),
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
        }
    }

    /// A `lifecycle` entry from its parts (the merge timer, abandon/reopen).
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
/// Belongs to one change; `sequence` totally orders the whole repo, `position`
/// orders one change. The flattened [`LogPayload`] contributes the `kind`
/// discriminant and the `payload` body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LogEntry {
    pub change_number: ChangeNumber,
    pub position: u64,
    pub sequence: u64,
    pub created_at: String,
    #[serde(flatten)]
    pub payload: LogPayload,
}
