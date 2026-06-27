//! Comment threads, reviewer drafts, and the selected-text range anchor they
//! share (docs/api.md "Comments").

use serde::{Deserialize, Serialize};

/// Selected-text anchor of a line comment: 1-based lines on the comment's
/// side, 0-based chars, `end_char` exclusive, `end_line` = the comment's
/// `line`. The JSON shape is these four fields. They are domain coordinates
/// (always non-negative), so the shape is `u64`; the server's `SQLite`
/// columns are signed, converted at the db boundary like every other id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentRange {
    pub start_line: u64,
    pub start_char: u64,
    pub end_line: u64,
    pub end_char: u64,
}
