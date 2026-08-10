//! The log: one entry per thing that happened to a change.

use serde::{Deserialize, Serialize};

/// The kind of one log entry.
///
/// The fold dispatches on it; the db `log.kind` TEXT column stores its
/// [`LogKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Revision,
    Review,
    Comment,
    Lifecycle,
}

impl LogKind {
    /// The persisted/wire spelling (db↔domain boundary).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LogKind::Revision => "revision",
            LogKind::Review => "review",
            LogKind::Comment => "comment",
            LogKind::Lifecycle => "lifecycle",
        }
    }
}

impl std::str::FromStr for LogKind {
    type Err = String;

    fn from_str(s: &str) -> Result<LogKind, String> {
        match s {
            "revision" => Ok(LogKind::Revision),
            "review" => Ok(LogKind::Review),
            "comment" => Ok(LogKind::Comment),
            "lifecycle" => Ok(LogKind::Lifecycle),
            other => Err(format!("unknown log entry kind {other:?}")),
        }
    }
}

/// What a `lifecycle` log entry records about a change.
///
/// The merge/abandon timer writes `merged`/`abandoned`; `nit reopen`
/// writes `reopened`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    Merged,
    Abandoned,
    Reopened,
}

impl LifecycleAction {
    /// The wire spelling (mirrors the serde renaming), for Value-free display.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleAction::Merged => "merged",
            LifecycleAction::Abandoned => "abandoned",
            LifecycleAction::Reopened => "reopened",
        }
    }
}
