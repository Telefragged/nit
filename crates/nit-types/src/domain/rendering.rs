//! Vocabulary a client draws with.

use serde::{Deserialize, Serialize};

/// Which region of the change graph a node sits in.
///
/// `open` ascends above the canonical HEAD, `head` is the HEAD anchor,
/// and `history` descends below it (merged commits, fading with depth).
/// The client styles a node by its `section` first (head → ring,
/// history → grey/fade), falling back to its `ChangeStatus` for open
/// nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GraphSection {
    Open,
    Head,
    History,
}

/// `DiffFile.status` — how a file changed between the two diffed trees.
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
/// `Raw` renders every line. `Outline` collapses every function body, so
/// that only signatures, doc-comments, types and fields remain — the change
/// read at the altitude of its API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DiffMode {
    #[default]
    Raw,
    Outline,
}

/// `Line.kind` — a diff line's role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Add,
    Del,
}
