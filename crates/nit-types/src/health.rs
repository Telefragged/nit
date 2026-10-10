//! The health probe.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct Health {
    pub status: String,
    /// The server's semver version. A build of a git checkout appends
    /// `+<sha>`, and `+<sha>.dirty` for an uncommitted tree.
    pub version: String,
}
