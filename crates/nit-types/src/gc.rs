//! Collecting closed changes and unreferenced attachments.

use serde::{Deserialize, Serialize};

/// The `retention_days` of a request that does not set it.
pub const DEFAULT_RETENTION_DAYS: u16 = 30;

/// `POST /api/gc` request body.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GcRequest {
    /// The age in days, counted from its last log entry, at which a closed
    /// change is deleted.
    ///
    /// Absent means [`DEFAULT_RETENTION_DAYS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u16>,
}

/// `POST /api/gc` response: what the collection deleted.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GcReport {
    /// The number of closed changes deleted.
    pub changes: u64,
    /// The number of attachment files deleted, the temp files of
    /// unfinished uploads included.
    pub attachments: u64,
    /// The total size of the deleted attachment files, in bytes.
    pub attachment_bytes: u64,
}
