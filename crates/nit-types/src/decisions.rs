//! The outcome of batch-submitting draft decisions.

use crate::domain::ChangeNumber;
use serde::{Deserialize, Serialize};

/// `POST /api/submit` response.
///
/// The query is the change list's (`repo`, `status`, `tag`, `change_id`),
/// and every change it picks publishes its draft decision, at the
/// change's latest revision. A change with no draft decision is left as
/// it is, comment drafts included.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BatchSubmitResult {
    /// Changes whose draft decision published.
    pub submitted: u64,
    /// Changes skipped (stale/terminal); their draft decision is kept.
    pub errors: Vec<SubmitError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SubmitError {
    pub change_number: ChangeNumber,
    pub message: String,
}
