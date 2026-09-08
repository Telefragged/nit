//! Websocket messages over `WS /api/stream`.
//!
//! The client sends one [`Subscription`]; the server answers with
//! [`StreamMessage`] frames.

use serde::{Deserialize, Serialize};

use crate::changes::ChangeQuery;
use crate::domain::ChangeProjection;
use crate::domain::LogEntry;

/// A client → server websocket message: what the socket follows.
///
/// The server first sends the [`ChangeProjection`] of every change
/// `query` picks now, then the stored entries with `sequence > after`
/// when `after` is given, then each new entry of a picked change. For a
/// new entry, `status` is the change's status right after it. A change
/// the socket meets for the first time, one that gets the tags from a
/// `tags` entry, arrives as its projection and then the entry.
///
/// A socket holds one subscription. A second one replaces the first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Subscription {
    pub query: ChangeQuery,
    /// Send the stored entries with a `sequence` greater than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub after: Option<u64>,
}

/// A server → client websocket message. Externally tagged, `snake_case`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StreamMessage {
    /// A picked change's folded projection, sent once, before its entries.
    Projection(ChangeProjection),
    /// One entry of a picked change, stored or live.
    ///
    /// Its `position` is below the projection's `entries_folded` when the
    /// projection already holds it.
    Entry(LogEntry),
}
