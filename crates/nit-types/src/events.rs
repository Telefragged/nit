//! Websocket messages over `WS /api/stream` (docs/api.md "Events"): the server
//! reads `ClientMsg` and writes bare [`LogEntry`](crate::log::LogEntry) frames,
//! the CLI follower does the reverse.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A client → server websocket message: subscribe a set of changes, each from
/// an idx. Externally tagged. Serde cannot deserialize integer map keys
/// through the content-buffering an internally-tagged/untagged enum needs,
/// so change IDs are `String` not `u64`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMsg {
    Subscribe(HashMap<String, u64>),
}
