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
///
/// [`as_str`]: LogKind::as_str
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Revisions,
    Review,
    Comment,
    Partial,
    ChainClosed,
}

impl LogKind {
    /// The persisted/wire spelling (the db↔domain boundary; mirrors the
    /// serde renaming).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LogKind::Revisions => "revisions",
            LogKind::Review => "review",
            LogKind::Comment => "comment",
            LogKind::Partial => "partial",
            LogKind::ChainClosed => "chain_closed",
        }
    }
}

impl std::str::FromStr for LogKind {
    type Err = String;

    fn from_str(s: &str) -> Result<LogKind, String> {
        match s {
            "revisions" => Ok(LogKind::Revisions),
            "review" => Ok(LogKind::Review),
            "comment" => Ok(LogKind::Comment),
            "partial" => Ok(LogKind::Partial),
            "chain_closed" => Ok(LogKind::ChainClosed),
            other => Err(format!("unknown log entry kind {other:?}")),
        }
    }
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

/// A change's wire status (docs/api.md state table). The fold keeps the
/// retained [`Status`](crate::review::Status) plus a separate orphaned flag
/// and collapses them into this for the wire ([`ChangeProj::wire_status`]).
///
/// [`ChangeProj::wire_status`]: crate::review::ChangeProj::wire_status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Pending,
    Approved,
    ChangesRequested,
    Commented,
    Orphaned,
}

/// A chain's derived, actionable state (docs/api.md state table). Computed
/// from the live changes by [`derive_state`](crate::review::derive_state);
/// it is informational on the wire, never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainState {
    WaitingForReview,
    AgentsTurn,
    Approved,
    Merged,
    Abandoned,
}

impl ChainState {
    /// Whether the agent has something to act on (`!= waiting_for_review`).
    #[must_use]
    pub fn actionable(self) -> bool {
        self != ChainState::WaitingForReview
    }
}

/// How a chain closed — the two terminal [`ChainStatus`] values, as the
/// `chain_closed` log payload carries them (docs/data-model.md "Payloads").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosedStatus {
    Merged,
    Abandoned,
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
