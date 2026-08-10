//! Websocket messages over `WS /api/stream`.
//!
//! The client picks one of two subscribe modes; the server answers with
//! [`StreamMessage`] frames — a `ChangeProjection` (projection mode) and/or
//! live log entries.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::ChangeNumber;
use crate::fold::ChangeProjection;
use crate::log::LogEntry;

/// A client → server websocket message. Externally tagged, `snake_case`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    /// Cursor replay (the CLI follower): `change_id` → from-position.
    ///
    /// The server replays each change's `[from, head)` backlog, then
    /// streams live. Integer map keys can't survive serde's tagged-enum
    /// content buffering, so the ids are `String`.
    Subscribe(HashMap<String, u64>),
    /// Projection mode (the web change page).
    ///
    /// For each change id the server folds a [`ChangeProjection`] projection and
    /// ships it, then attaches the live tail past the projection's
    /// high-water mark. A `Vec` has no map keys, so the ids stay `u64`
    /// (unlike `Subscribe`).
    SubscribeProjection(Vec<ChangeNumber>),
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
