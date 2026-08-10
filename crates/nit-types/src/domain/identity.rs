//! The names a thing is known by.

use serde::{Deserialize, Serialize};

/// A change's identity: its `Change-Id` trailer, verbatim.
///
/// It is carried in the commit message and survives the rewrites review
/// provokes, which is what binds a new revision to the change it revises;
/// a commit sha does not.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeId(pub String);

impl ChangeId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ChangeId {
    fn from(s: String) -> ChangeId {
        ChangeId(s)
    }
}

impl From<&str> for ChangeId {
    fn from(s: &str) -> ChangeId {
        ChangeId(s.to_string())
    }
}

impl PartialEq<&str> for ChangeId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl std::fmt::Display for ChangeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A git object name, in full: 40 hex characters.
///
/// Clients truncate it for display; nothing but display uses the short
/// form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Sha(pub String);

impl Sha {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Sha {
    fn from(s: String) -> Sha {
        Sha(s)
    }
}

impl From<&str> for Sha {
    fn from(s: &str) -> Sha {
        Sha(s.to_string())
    }
}

impl PartialEq<&str> for Sha {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl std::fmt::Display for Sha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which version of a change: 0-based, in the order the revisions were
/// observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RevisionNumber(pub u64);

impl RevisionNumber {
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }

    /// The revision before this one, or `None` at a change's first.
    #[must_use]
    pub fn previous(self) -> Option<RevisionNumber> {
        self.0.checked_sub(1).map(RevisionNumber)
    }
}

impl From<u64> for RevisionNumber {
    fn from(n: u64) -> RevisionNumber {
        RevisionNumber(n)
    }
}

impl std::fmt::Display for RevisionNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for RevisionNumber {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<RevisionNumber, std::num::ParseIntError> {
        s.parse().map(RevisionNumber)
    }
}
