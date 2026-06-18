//! The shared closed vocabularies of nit: the small sets of named values —
//! sides, authors, verdicts, statuses, kinds — that appear in both the
//! domain (`crate::review` fold) and the wire (`crate::api::types`), and on
//! the CLI.
//!
//! **Discipline: a closed set of values is an `enum`, never a `String`.**
//! Every value that can only be one of a fixed list lives here as a serde
//! enum whose `rename`/`rename_all` fixes its on-the-wire spelling, so the
//! *same* type is the domain value, the JSON shape (docs/api.md), and the
//! parsed CLI input. The payoff is concrete: an exhaustive `match` instead
//! of a `_ =>` fallthrough, no `as_str`/`from_str` round-tripping at the
//! domain↔wire boundary, and — because `#[serde(deny_unknown…)]`-style
//! rejection is automatic for enums — an unknown value is a clean
//! deserialization error (a 400 through `AppJson`), not a string that flows
//! deeper before something notices. New enumerated fields are added here and
//! referenced from both sides; they are never reintroduced as `String`.
//!
//! Serde renamings reproduce the exact wire spellings documented in
//! docs/api.md, so swapping a `String` field for one of these enums is not a
//! wire change.

use serde::{Deserialize, Serialize};

/// Which tree of a revision a line comment is anchored to (docs/api.md
/// "Comment placement"): `new` is the revision's commit tree, `old` its
/// parent tree. Defaults to `new` where a request omits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Old,
    #[default]
    New,
}

impl Side {
    /// The persisted/wire spelling — also the `drafts.side` column value
    /// (the db↔domain boundary; mirrors the serde renaming).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Old => "old",
            Side::New => "new",
        }
    }
}

impl std::str::FromStr for Side {
    type Err = String;

    fn from_str(s: &str) -> Result<Side, String> {
        match s {
            "old" => Ok(Side::Old),
            "new" => Ok(Side::New),
            other => Err(format!(
                "invalid side {other:?} (expected \"old\" or \"new\")"
            )),
        }
    }
}

/// Who wrote a comment (docs/api.md "Comment placement"): a `reviewer`
/// verdict comment, or an `agent` note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    Reviewer,
    Agent,
}

/// The kind of one log entry (docs/data-model.md "The log"). The fold
/// dispatches on it; the db `log.kind` TEXT column stores its [`as_str`].
/// Each entry belongs to one **change**: a `revision` records a new
/// commit-sha for the change, a `lifecycle` records a merge/abandon/reopen.
///
/// [`as_str`]: LogKind::as_str
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Revision,
    Review,
    Comment,
    Lifecycle,
    Partial,
}

impl LogKind {
    /// The persisted/wire spelling (the db↔domain boundary; mirrors the
    /// serde renaming).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LogKind::Revision => "revision",
            LogKind::Review => "review",
            LogKind::Comment => "comment",
            LogKind::Lifecycle => "lifecycle",
            LogKind::Partial => "partial",
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
            "partial" => Ok(LogKind::Partial),
            other => Err(format!("unknown log entry kind {other:?}")),
        }
    }
}

/// What a `lifecycle` log entry records about a change (docs/data-model.md
/// "Payloads"). The merge/abandon timer writes `merged`/`abandoned`;
/// `nit reopen` writes `reopened`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    Merged,
    Abandoned,
    Reopened,
}

/// A reviewer's verdict on one change (docs/api.md "Reviews"). Maps to a
/// change [`Status`](crate::review::Status) when folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

/// A change's displayed status at a pinned revision (docs/api.md state
/// table): the verdict-derived [`Status`](crate::review::Status) under the
/// lifecycle overlay (`merged` for the landed patchset, `abandoned`
/// change-wide). Per `(change, revision)`, never a change-wide scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Pending,
    Approved,
    ChangesRequested,
    Commented,
    Merged,
    Abandoned,
}

impl ChangeStatus {
    /// Terminal for review (the lifecycle closed it).
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, ChangeStatus::Merged | ChangeStatus::Abandoned)
    }
}

/// A chain's derived, actionable state (docs/api.md state table). Computed
/// at read time from the path's members ([`derive_state`](crate::chain::derive_state));
/// it is informational on the wire, never stored. Abandonment is
/// derivation-inert — there is no abandoned chain state (an abandoned member is
/// excluded from the rollup; the agent reasons about its per-change status).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainState {
    Merged,
    AgentsTurn,
    WaitingForReview,
    Approved,
}

impl ChainState {
    /// Whether the agent has something to act on (`!= waiting_for_review`).
    #[must_use]
    pub fn actionable(self) -> bool {
        self != ChainState::WaitingForReview
    }
}

/// `DiffFile.status` — how a file changed between the two diffed trees
/// (docs/api.md "Diff").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// `Line.kind` — a diff line's role (docs/api.md "Diff").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Add,
    Del,
}
