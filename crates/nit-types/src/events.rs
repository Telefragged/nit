//! Websocket messages over `WS /api/stream` (docs/api.md "Events"). The client
//! picks one of two subscribe modes; the server answers with [`StreamMsg`]
//! frames — a `ChangeProj` snapshot (snapshot mode) and/or live log entries.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::fold::ChangeProj;
use crate::log::LogEntry;

/// A client → server websocket message. Externally tagged, `snake_case`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMsg {
    /// Cursor replay (the CLI follower): `change_id` → from-idx; the server
    /// replays each change's `[from, head)` backlog, then streams live. Integer
    /// map keys can't survive serde's tagged-enum content buffering, so the ids
    /// are `String`.
    Subscribe(HashMap<String, u64>),
    /// Snapshot mode (the web change page): for each change id the server folds
    /// a [`ChangeProj`] snapshot and ships it, then attaches the live tail past
    /// the snapshot's high-water mark. A `Vec` has no map keys, so the ids stay
    /// `u64` (unlike `Subscribe`).
    SubscribeSnapshot(Vec<u64>),
}

/// A server → client websocket message (docs/api.md "Events"). Externally
/// tagged, `snake_case`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamMsg {
    /// The change's folded projection at subscribe time — the snapshot a
    /// snapshot-mode follower resumes from. Sent once per change, before its
    /// live tail.
    Snapshot(ChangeProj),
    /// One live (or replayed-backlog) log entry, past the snapshot's
    /// `entries_folded` for a snapshot-mode follower.
    Entry(LogEntry),
}
