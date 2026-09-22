//! The error envelope: every non-2xx response is `{"error": "..."}`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ApiError {
    pub error: String,
}
