//! Porting: translating a thread's anchor from one revision to another.

use serde::{Deserialize, Serialize};

use super::Anchor;

/// Where a thread was written: the anchor it was given.
///
/// A port starts here and ends in a [`PortedComment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadOrigin {
    pub thread_id: u64,
    pub anchor: Anchor,
}

/// A thread's anchor ported to a revision it was not written on.
///
/// The thread keeps the anchor it was written with. `anchor` is where that
/// one lands in the trees of another revision, derived from the diff
/// between them: the same lines when the diff left them alone, shifted
/// lines when it inserted or deleted lines above them, and the next place
/// up (the file, then the change) when it rewrote those lines or dropped
/// the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct PortedComment {
    pub thread_id: u64,
    pub anchor: Anchor,
}
