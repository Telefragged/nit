//! Vocabulary a client draws with.

use serde::{Deserialize, Serialize};

/// Which region of the change graph a node sits in.
///
/// `open` ascends above the canonical HEAD, `head` is the HEAD anchor,
/// and `history` descends below it: merged commits, oldest deepest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GraphSection {
    Open,
    Head,
    History,
}

/// How a file changed between the two diffed trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// Whether whitespace is part of a line's identity.
///
/// `Compare` compares a line as its whole text. `Ignore` compares two
/// lines as one line when they hold the same characters apart from
/// whitespace, so the diff does not show a block that was only
/// re-indented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Whitespace {
    #[default]
    Compare,
    Ignore,
}

/// How a reviewer asked for a diff to be drawn.
///
/// Each setting is independent, and a request that omits one takes its
/// default.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
    utoipa::IntoParams,
)]
#[into_params(parameter_in = Query)]
pub struct DiffView {
    #[serde(default)]
    pub whitespace: Whitespace,
}

/// A diff line's role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Add,
    Del,
}
