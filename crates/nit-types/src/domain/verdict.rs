//! What a reviewer concluded, and the states derived from it.

use serde::{Deserialize, Serialize};

use super::LifecycleAction;

/// A reviewer's verdict on one change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

impl Verdict {
    /// The wire spelling (mirrors the serde renaming), for Value-free display.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Approve => "approve",
            Verdict::RequestChanges => "request_changes",
            Verdict::Comment => "comment",
        }
    }
}

/// A reviewer's **draft** decision on a change.
///
/// The review modal's single set of choices, drafted in `draft_reviews`
/// and published on batch submit, where it translates back to a
/// [`Verdict`] or a [`LifecycleAction`] ([`Decision::as_verdict`],
/// [`Decision::as_lifecycle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    RequestChanges,
    Comment,
    Abandon,
    Reopen,
}

impl Decision {
    #[must_use]
    pub fn as_verdict(self) -> Option<Verdict> {
        match self {
            Decision::Approve => Some(Verdict::Approve),
            Decision::RequestChanges => Some(Verdict::RequestChanges),
            Decision::Comment => Some(Verdict::Comment),
            Decision::Abandon | Decision::Reopen => None,
        }
    }

    #[must_use]
    pub fn as_lifecycle(self) -> Option<LifecycleAction> {
        match self {
            Decision::Abandon => Some(LifecycleAction::Abandoned),
            Decision::Reopen => Some(LifecycleAction::Reopened),
            Decision::Approve | Decision::RequestChanges | Decision::Comment => None,
        }
    }

    /// The persisted/wire spelling — the `draft_reviews.decision` column value
    /// (db↔domain boundary).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::RequestChanges => "request_changes",
            Decision::Comment => "comment",
            Decision::Abandon => "abandon",
            Decision::Reopen => "reopen",
        }
    }
}

impl std::str::FromStr for Decision {
    type Err = String;

    fn from_str(s: &str) -> Result<Decision, String> {
        match s {
            "approve" => Ok(Decision::Approve),
            "request_changes" => Ok(Decision::RequestChanges),
            "comment" => Ok(Decision::Comment),
            "abandon" => Ok(Decision::Abandon),
            "reopen" => Ok(Decision::Reopen),
            other => Err(format!("unknown decision {other:?}")),
        }
    }
}

/// A change's displayed status at a pinned revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
    /// The persisted/wire spelling — the denormalized `changes.status` column
    /// value (db↔domain boundary).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeStatus::Pending => "pending",
            ChangeStatus::Approved => "approved",
            ChangeStatus::ChangesRequested => "changes_requested",
            ChangeStatus::Commented => "commented",
            ChangeStatus::Merged => "merged",
            ChangeStatus::Abandoned => "abandoned",
        }
    }
}

impl std::str::FromStr for ChangeStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<ChangeStatus, String> {
        match s {
            "pending" => Ok(ChangeStatus::Pending),
            "approved" => Ok(ChangeStatus::Approved),
            "changes_requested" => Ok(ChangeStatus::ChangesRequested),
            "commented" => Ok(ChangeStatus::Commented),
            "merged" => Ok(ChangeStatus::Merged),
            "abandoned" => Ok(ChangeStatus::Abandoned),
            other => Err(format!("unknown change status {other:?}")),
        }
    }
}

impl From<Verdict> for ChangeStatus {
    /// The review status a verdict produces.
    ///
    /// It is the status before the lifecycle overlay (`merged`/`abandoned`)
    /// the server's fold layers on top.
    fn from(verdict: Verdict) -> ChangeStatus {
        match verdict {
            Verdict::Approve => ChangeStatus::Approved,
            Verdict::RequestChanges => ChangeStatus::ChangesRequested,
            Verdict::Comment => ChangeStatus::Commented,
        }
    }
}

/// A chain's derived, actionable state.
///
/// Computed at read time from the path's members (the server's
/// `chain::derive_state`); it is informational on the wire, never stored.
/// Abandonment is derivation-inert — there is no abandoned chain state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChainState {
    Merged,
    AuthorsTurn,
    WaitingForReview,
    Approved,
}

impl ChainState {
    /// The wire spelling, for Value-free display in the CLI.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChainState::Merged => "merged",
            ChainState::AuthorsTurn => "authors_turn",
            ChainState::WaitingForReview => "waiting_for_review",
            ChainState::Approved => "approved",
        }
    }
}
