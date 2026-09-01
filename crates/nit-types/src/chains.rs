//! The chain list's response body.

use serde::{Deserialize, Serialize};

use crate::domain::Chain;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainList {
    pub chains: Vec<Chain>,
}
