//! The names a thing is known by.

use serde::{Deserialize, Serialize};

/// A change's identity: its `Change-Id` trailer, verbatim.
///
/// It is carried in the commit message and survives the rewrites review
/// provokes, which is what binds a new revision to the change it revises;
/// a commit sha does not.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeId(String);

impl ChangeId {
    /// A `Change-Id` trailer value: `I` followed by 40 hex characters.
    ///
    /// # Errors
    ///
    /// [`ChangeIdError`], naming the rule the input broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::ChangeId;
    ///
    /// assert!(ChangeId::new("I88362667d0327f71c84fecf3a09d042e2d4a1cba").is_ok());
    /// assert!(ChangeId::new("my-feature").is_err());
    /// ```
    pub fn new(change_id: impl Into<String>) -> Result<ChangeId, ChangeIdError> {
        let change_id = change_id.into();
        let Some(hex) = change_id.strip_prefix('I') else {
            return Err(ChangeIdError::Prefix);
        };
        if hex.len() != 40 {
            return Err(ChangeIdError::Length(hex.len()));
        }
        if let Some(c) = hex.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(ChangeIdError::NotHex(c));
        }
        Ok(ChangeId(change_id))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a string is not a [`ChangeId`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChangeIdError {
    #[error("a Change-Id starts with 'I'")]
    Prefix,
    #[error("a Change-Id holds 40 hex characters after its 'I', not {0}")]
    Length(usize),
    #[error("a Change-Id is hexadecimal after its 'I', and '{0}' is not")]
    NotHex(char),
}

impl TryFrom<String> for ChangeId {
    type Error = ChangeIdError;

    fn try_from(change_id: String) -> Result<ChangeId, ChangeIdError> {
        ChangeId::new(change_id)
    }
}

// Serialized by hand rather than `#[serde(transparent)]`, which serde
// refuses to combine with the `try_from` that gates the way in.
impl Serialize for ChangeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
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
/// Only display ever shortens it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Sha(String);

impl Sha {
    /// The name of a git object, checked against git's own spelling of one.
    ///
    /// # Errors
    ///
    /// [`ShaError`], naming the rule the input broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::Sha;
    ///
    /// assert!(Sha::new("368a08a0d1d5f1e0a4a02b8fd8b3fbb1c5c3e1a9").is_ok());
    /// assert!(Sha::new("368a08a0").is_err());
    /// ```
    pub fn new(sha: impl Into<String>) -> Result<Sha, ShaError> {
        let sha = sha.into();
        if sha.len() != 40 {
            return Err(ShaError::Length(sha.len()));
        }
        if let Some(c) = sha.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(ShaError::NotHex(c));
        }
        Ok(Sha(sha))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a string is not a [`Sha`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShaError {
    #[error("a git object name is 40 characters, not {0}")]
    Length(usize),
    #[error("a git object name is hexadecimal, and '{0}' is not")]
    NotHex(char),
}

impl TryFrom<String> for Sha {
    type Error = ShaError;

    fn try_from(sha: String) -> Result<Sha, ShaError> {
        Sha::new(sha)
    }
}

// Serialized by hand rather than `#[serde(transparent)]`, which serde
// refuses to combine with the `try_from` that gates the way in.
impl Serialize for Sha {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
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
pub struct RevisionNumber(u64);

impl RevisionNumber {
    #[must_use]
    pub const fn new(number: u64) -> RevisionNumber {
        RevisionNumber(number)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The revision before this one, or `None` at a change's first.
    #[must_use]
    pub fn previous(self) -> Option<RevisionNumber> {
        self.0.checked_sub(1).map(RevisionNumber)
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

/// A change's number: the handle nit assigns it and everything carries.
///
/// Scoped to one nit instance, unlike the [`ChangeId`] that travels in the
/// commit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangeNumber(u64);

impl ChangeNumber {
    #[must_use]
    pub const fn new(number: u64) -> ChangeNumber {
        ChangeNumber(number)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ChangeNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ChangeNumber {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<ChangeNumber, std::num::ParseIntError> {
        s.parse().map(ChangeNumber)
    }
}
