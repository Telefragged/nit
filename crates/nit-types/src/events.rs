//! Websocket messages over `WS /api/stream`.
//!
//! The client picks one of two subscribe modes; the server answers with
//! [`StreamMessage`] frames — a `ChangeProjection` (projection mode) and/or
//! live log entries.

use serde::{Deserialize, Serialize};

use crate::domain::ChangeNumber;
use crate::domain::ChangeProjection;
use crate::domain::LogEntry;
use crate::domain::Tags;

/// A client → server websocket message. Externally tagged, `snake_case`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    /// Projection mode (the web change page).
    ///
    /// For each change number the server folds a [`ChangeProjection`] projection and
    /// ships it, then attaches the live tail past the projection's
    /// high-water mark.
    SubscribeProjection(Vec<ChangeNumber>),
    /// Tag mode (the CLI follower): every change in `repo` that has `tags`.
    ///
    /// First the server sends every stored entry with `sequence > after`
    /// whose change has the tags. Then it sends each new entry whose change
    /// has the tags at the moment the entry is written. A change without
    /// the tags gets them from a `tags` entry. The server sends that entry
    /// and every later one, but not the earlier ones, so a client that
    /// needs the earlier ones reads `GET /api/log`. A socket holds one tag
    /// subscription. A second one replaces the first.
    SubscribeTagged {
        repo: u64,
        tags: Tags,
        /// Send entries with a `sequence` greater than this.
        after: u64,
    },
}

/// A server → client websocket message. Externally tagged, `snake_case`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StreamMessage {
    /// The change's folded projection at subscribe time.
    ///
    /// The projection a projection-mode follower resumes from. Sent once per
    /// change, before its live tail.
    Projection(ChangeProjection),
    /// One live (or replayed-backlog) log entry.
    ///
    /// Past the projection's `entries_folded` for a projection-mode follower.
    Entry(LogEntry),
}
