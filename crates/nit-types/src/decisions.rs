//! The outcome of batch-submitting a chain's staged decisions.

use serde::{Deserialize, Serialize};

/// `POST /api/chains/{id}/submit` response.
///
/// The outcome of publishing every chain member's staged decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BatchSubmitResult {
    /// Members whose staged decision published.
    pub submitted: u64,
    /// Members skipped (stale/terminal); their staged decision is kept.
    pub errors: Vec<SubmitError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SubmitError {
    pub change_id: u64,
    pub message: String,
}
