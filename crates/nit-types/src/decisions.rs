//! The outcome of batch-submitting a chain's draft decisions.

use crate::domain::ChangeNumber;
use serde::{Deserialize, Serialize};

/// `POST /api/chains/{id}/submit` response.
///
/// The outcome of publishing every chain member's draft decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BatchSubmitResult {
    /// Members whose draft decision published.
    pub submitted: u64,
    /// Members skipped (stale/terminal); their draft decision is kept.
    pub errors: Vec<SubmitError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SubmitError {
    pub change_id: ChangeNumber,
    pub message: String,
}
