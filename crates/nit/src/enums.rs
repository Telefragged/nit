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

/// A reviewer's verdict on one change (docs/api.md "Reviews"). Maps to a
/// change [`Status`](crate::review::Status) when folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
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
