//! The health probe.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Health {
    pub status: String,
    pub version: String,
}
