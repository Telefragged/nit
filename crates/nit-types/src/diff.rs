//! File diffs and their hunks/lines (docs/api.md "Diff").

use serde::{Deserialize, Serialize};

use crate::enums::{FileStatus, LineKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    pub files: Vec<DiffFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFile {
    /// New path (old path when deleted).
    pub path: String,
    /// Only set for renames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub binary: bool,
    pub additions: u64,
    pub deletions: u64,
    /// Empty when binary.
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: u64,
    pub old_lines: u64,
    pub new_start: u64,
    pub new_lines: u64,
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line {
    pub kind: LineKind,
    /// Old line number; absent for add.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<u64>,
    /// New line number; absent for del.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<u64>,
    /// Changed by a rebase, not the agent (docs/api.md "Rebase-aware
    /// interdiffs").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub drift: bool,
    /// Without trailing newline.
    pub text: String,
}
