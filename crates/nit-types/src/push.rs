//! The push endpoint (docs/api.md "Push").

use serde::{Deserialize, Serialize};

use crate::enums::ChangeStatus;

/// `POST /api/push` request (this is `nit push`). The repo must already be
/// registered (`nit repo create`); the canonical branch is its stored
/// `base_ref`, so push takes no base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub git_dir: String,
    /// Any ref or rev, resolved to a commit at push time.
    pub tip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResult {
    /// The pushed tip change, at the revision this push gave it. Always
    /// present — a push that walks to nothing is rejected (409). Read the
    /// derived chain back with `GET /api/chains/{tip_change.change_id}`.
    pub tip_change: TipChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TipChange {
    pub change_id: u64,
    pub change_key: String,
    pub revision: u64,
    pub status: ChangeStatus,
}
