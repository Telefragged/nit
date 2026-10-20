//! Uploading attachments and putting them on revisions.

use serde::{Deserialize, Serialize};

use crate::domain::AttachmentHash;

/// `PUT /api/attachments` response: the stored blob.
///
/// The upload stores bytes only. An attachment exists once an attach
/// entry names this hash on a revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct UploadedAttachment {
    pub hash: AttachmentHash,
}
