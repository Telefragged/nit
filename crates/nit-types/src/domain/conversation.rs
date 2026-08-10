//! The review conversation: threads, where they anchor, and the
//! reviewer's unpublished drafts.

use serde::{Deserialize, Serialize};

/// Which tree of a revision a line comment is anchored to.
///
/// `new` is the revision's commit tree, `old` its parent tree. Defaults
/// to `new` where a request omits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Old,
    #[default]
    New,
}

impl Side {
    /// The persisted/wire spelling — the `drafts.side` column value
    /// (db↔domain boundary).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Old => "old",
            Side::New => "new",
        }
    }
}

impl std::str::FromStr for Side {
    type Err = String;

    fn from_str(s: &str) -> Result<Side, String> {
        match s {
            "old" => Ok(Side::Old),
            "new" => Ok(Side::New),
            other => Err(format!(
                "invalid side {other:?} (expected \"old\" or \"new\")"
            )),
        }
    }
}
