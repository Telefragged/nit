//! Websocket messages over `WS /api/stream` (docs/api.md "Events"). Both
//! directions are derived on every message: the server reads `ClientMsg` and
//! writes `StreamMsg`, the CLI follower does the reverse.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::log::LogEntry;

/// A server → client websocket message: a tagged [`LogEntry`], or the
/// out-of-log `new_parent` advisory. `untagged` so an entry serializes as its
/// bare fields and `new_parent` as `{"new_parent": {…}}` — the client tells
/// them apart by the `new_parent` key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamMsg {
    Entry(LogEntry),
    NewParent { new_parent: NewParent },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewParent {
    /// The child end of the newly established edge (a re-rooted change, or a
    /// brand-new child stacked on `parent`).
    pub of: u64,
    /// The parent `of` now sits on.
    pub parent: u64,
}

/// A client → server websocket message: subscribe a set of changes, each from
/// an idx. Externally tagged. Serde cannot deserialize integer map keys
/// through the content-buffering an internally-tagged/untagged enum needs,
/// so change IDs are `String` not `u64`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMsg {
    Subscribe(HashMap<String, u64>),
}
