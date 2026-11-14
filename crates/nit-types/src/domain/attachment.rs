//! Attachments: files stored on a change beside its commit.

use serde::{Deserialize, Serialize};

use super::ChangeVersion;
use super::RevisionNumber;

/// The BLAKE3 hash of an attachment's bytes: 64 lowercase hex characters.
///
/// It identifies the bytes, so two attachments with equal hashes have
/// the same content.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, utoipa::ToSchema)]
#[serde(try_from = "String")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AttachmentHash(String);

impl AttachmentHash {
    /// A BLAKE3 hash, in the lowercase hex that BLAKE3 prints.
    ///
    /// # Errors
    ///
    /// [`AttachmentHashError`], which identifies the rule the input broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::AttachmentHash;
    ///
    /// assert!(AttachmentHash::new("af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262").is_ok());
    /// assert!(AttachmentHash::new("AF1349B9").is_err());
    /// ```
    pub fn new(hash: impl Into<String>) -> Result<AttachmentHash, AttachmentHashError> {
        let hash = hash.into();
        if hash.len() != 64 {
            return Err(AttachmentHashError::Length(hash.len()));
        }
        if let Some(c) = hash.chars().find(|c| !matches!(c, '0'..='9' | 'a'..='f')) {
            return Err(AttachmentHashError::NotLowerHex(c));
        }
        Ok(AttachmentHash(hash))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a string is not an [`AttachmentHash`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttachmentHashError {
    #[error("an attachment hash is 64 characters, not {0}")]
    Length(usize),
    #[error("an attachment hash is lowercase hexadecimal, and '{0}' is not")]
    NotLowerHex(char),
}

impl TryFrom<String> for AttachmentHash {
    type Error = AttachmentHashError;

    fn try_from(hash: String) -> Result<AttachmentHash, AttachmentHashError> {
        AttachmentHash::new(hash)
    }
}

// serde rejects `#[serde(transparent)]` together with `try_from`, so
// this impl serializes by hand.
impl Serialize for AttachmentHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl std::fmt::Display for AttachmentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of an attachment on a revision: a relative path.
///
/// Its segments are separated by `/`. Within one revision a name
/// identifies one attachment, and across revisions it pairs the versions
/// of one attachment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, utoipa::ToSchema)]
#[serde(try_from = "String")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AttachmentName(String);

impl AttachmentName {
    /// A relative path whose every segment is a file or directory name.
    ///
    /// # Errors
    ///
    /// [`AttachmentNameError`], which identifies the rule the input broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::AttachmentName;
    ///
    /// assert!(AttachmentName::new("web/screenshots/review.png").is_ok());
    /// assert!(AttachmentName::new("/etc/passwd").is_err());
    /// assert!(AttachmentName::new("a/../b").is_err());
    /// ```
    pub fn new(name: impl Into<String>) -> Result<AttachmentName, AttachmentNameError> {
        let name = name.into();
        if name.contains('\0') {
            return Err(AttachmentNameError::Nul);
        }
        if let Some(segment) = name
            .split('/')
            .find(|s| s.is_empty() || *s == "." || *s == "..")
        {
            return Err(AttachmentNameError::Segment(segment.to_string()));
        }
        Ok(AttachmentName(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a string is not an [`AttachmentName`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttachmentNameError {
    #[error("an attachment name holds no NUL")]
    Nul,
    #[error("an attachment name is a relative path, and {0:?} is not one of its segments")]
    Segment(String),
}

impl TryFrom<String> for AttachmentName {
    type Error = AttachmentNameError;

    fn try_from(name: String) -> Result<AttachmentName, AttachmentNameError> {
        AttachmentName::new(name)
    }
}

// serde rejects `#[serde(transparent)]` together with `try_from`, so
// this impl serializes by hand.
impl Serialize for AttachmentName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl std::fmt::Display for AttachmentName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// An `attach` entry: one attachment put on one version of a change.
///
/// An entry whose `name` matches an attachment already on the version
/// replaces it.
///
/// # Examples
///
/// An entry with a `revision` field puts the attachment on that revision:
///
/// ```rust
/// use nit_types::domain::{AttachPayload, ChangeVersion, RevisionNumber};
///
/// let hash = "a".repeat(64);
/// let stored: AttachPayload = serde_json::from_value(
///     serde_json::json!({ "revision": 1, "name": "plan.md", "hash": hash }),
/// )
/// .unwrap();
/// assert_eq!(stored.version, ChangeVersion::Revision(RevisionNumber::new(1)));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "AttachSpelling")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct AttachPayload {
    pub version: ChangeVersion,
    pub name: AttachmentName,
    pub hash: AttachmentHash,
}

/// An [`AttachPayload`] as the append-only log stores it: an entry
/// places the attachment with a `version` or with a `revision` field.
#[derive(Deserialize)]
struct AttachSpelling {
    #[serde(flatten)]
    on: On,
    name: AttachmentName,
    hash: AttachmentHash,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum On {
    Version(ChangeVersion),
    Revision(RevisionNumber),
}

impl From<AttachSpelling> for AttachPayload {
    fn from(AttachSpelling { on, name, hash }: AttachSpelling) -> AttachPayload {
        let version = match on {
            On::Version(version) => version,
            On::Revision(revision) => ChangeVersion::Revision(revision),
        };
        AttachPayload {
            version,
            name,
            hash,
        }
    }
}

/// One attachment of a change version, as the fold leaves it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct AttachmentProjection {
    pub name: AttachmentName,
    pub hash: AttachmentHash,
}

/// An attachment as one version of a change shows it.
///
/// A revision shows, for each name, the attachment of the latest revision
/// at or below it that has that name. The base of a revision shows the
/// same for the bases of those revisions. So an attachment stays in effect
/// until a later revision, or the base of a later revision, replaces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct EffectiveAttachment {
    pub name: AttachmentName,
    pub hash: AttachmentHash,
    /// The revision that it, or its base, is attached to, at or below the
    /// revision of the version that shows it.
    pub revision: RevisionNumber,
}
