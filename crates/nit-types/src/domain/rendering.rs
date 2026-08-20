//! Vocabulary a client draws with.

use serde::{Deserialize, Serialize};

/// Which region of the change graph a node sits in.
///
/// `open` ascends above the canonical HEAD, `head` is the HEAD anchor,
/// and `history` descends below it: merged commits, oldest deepest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GraphSection {
    Open,
    Head,
    History,
}

/// How a file changed between the two diffed trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// How much of a diff is rendered.
///
/// `Full` renders every line the change touched. `Outline` collapses every
/// function body and drops every import, so that only signatures,
/// doc-comments, types and fields remain — the change read at the altitude
/// of its API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DiffMode {
    #[default]
    Full,
    Outline,
}

/// A diff line's role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Add,
    Del,
}
