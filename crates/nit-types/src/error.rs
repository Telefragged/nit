//! The error envelope: every non-2xx response is `{"error": "..."}`
//! (docs/api.md).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}
