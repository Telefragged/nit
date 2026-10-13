//! Websocket messages over `WS /api/stream`.
//!
//! The client sends a [`Subscription`]. The server answers with
//! [`StreamMessage`] frames.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::changes::ChangeQuery;
use crate::domain::ChangeNumber;
use crate::domain::ChangeProjection;
use crate::domain::LogEntry;

/// A client → server websocket message: the changes that the socket
/// follows.
///
/// The server keeps the position of each followed change. After the
/// subscription and after each committed transaction, the server runs
/// `query` again and compares the result with the positions:
///
/// - For a change that `query` matches for the first time, the server
///   sends what `start` names.
/// - For a followed change, the server sends each entry from its
///   position on, in `position` order.
/// - For a followed change that `query` no longer matches, the server
///   sends `left`.
///
/// A socket holds one subscription. A second subscription replaces the
/// first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct Subscription {
    pub query: ChangeQuery,
    /// The changes that the client holds, each with the `position` of the
    /// next entry that the client lacks.
    ///
    /// The position of a projection is its `entries_folded`. The server
    /// sends the entries of these changes from the given position on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(
        feature = "ts",
        ts(as = "Option<BTreeMap<ChangeNumber, u64>>", optional)
    )]
    pub positions: BTreeMap<ChangeNumber, u64>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<Start>", optional))]
    pub start: Start,
}

/// How the server starts a matched change that the `positions` of a
/// [`Subscription`] lack.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// The projection of the change, then the entries after it.
    #[default]
    Projection,
    /// Every entry of the change, from position 0.
    Log,
}

/// A server → client websocket message. Externally tagged, `snake_case`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamMessage {
    /// The projection of a change that the query matches, sent before the
    /// entries of the change.
    Projection(ChangeProjection),
    /// An entry of a followed change, at the next position of the change.
    Entry(LogEntry),
    /// A followed change that the query no longer matches.
    Left(ChangeNumber),
}
