//! Uploading attachments.

use serde::{Deserialize, Serialize};

use crate::domain::AttachmentHash;

/// The largest attachment that reads as text, in bytes.
///
/// The server snapshots a line and ports a thread only on text this size
/// or smaller. A larger attachment shows no lines, so no thread anchors on
/// a line the server could not read.
pub const MAX_TEXT_SIZE: u64 = 1024 * 1024;

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
