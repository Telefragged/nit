//! The push endpoint.

use serde::{Deserialize, Serialize};

use crate::domain::ChangeId;
use crate::domain::ChangeNumber;
use crate::domain::ChangeStatus;
use crate::domain::RevisionNumber;

/// `POST /api/push` request (this is `nit push`).
///
/// The repo must already be registered (`nit repo create`); the canonical
/// branch is its stored `canonical_ref`, so push takes no base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub git_dir: String,
    /// Any ref or revision, resolved to a commit at push time.
    pub tip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResult {
    /// The repo the push registered the changes in.
    pub repo: u64,
    /// Every change the push walked, base first, the tip last.
    ///
    /// Never empty: a push that walks to nothing is rejected (409).
    pub changes: Vec<PushedChange>,
}

/// One change a push walked, at the revision the push left it at.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushedChange {
    pub change_number: ChangeNumber,
    pub change_id: ChangeId,
    pub revision: RevisionNumber,
    pub status: ChangeStatus,
}
