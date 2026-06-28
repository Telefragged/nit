//! The outcome of batch-submitting a chain's staged decisions (docs/api.md
//! "Chains" → submit).

use serde::{Deserialize, Serialize};

/// `POST /api/chains/{id}/submit` response — the outcome of publishing every
/// chain member's staged decision (docs/api.md "Chains").
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
