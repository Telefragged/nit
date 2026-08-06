//! The labels a push stamps on a change.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A tag set: one value per key.
///
/// The map orders by key, so a set serializes stably and two sets
/// compare verbatim. Only a [`Tag`] enters a set, and that holds for a
/// set arriving over the wire, so every pair in one meets the vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "BTreeMap<String, String>")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Tags(BTreeMap<String, String>);

impl Tags {
    /// An empty set.
    #[must_use]
    pub const fn new() -> Tags {
        Tags(BTreeMap::new())
    }

    /// Lays a later set over this one.
    ///
    /// A key that `later` names takes its value. A key that `later`
    /// omits keeps the value it had, so a tag follows a change forward
    /// without a restatement.
    pub fn overlay(&mut self, later: &Tags) {
        self.0
            .extend(later.0.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    /// The value `key` carries, if the set holds it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// How many keys the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The tags, ascending by key.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

impl FromIterator<Tag> for Tags {
    fn from_iter<I: IntoIterator<Item = Tag>>(tags: I) -> Tags {
        Tags(tags.into_iter().map(|t| (t.key, t.value)).collect())
    }
}

impl TryFrom<BTreeMap<String, String>> for Tags {
    type Error = TagError;

    fn try_from(pairs: BTreeMap<String, String>) -> Result<Tags, TagError> {
        pairs
            .into_iter()
            .map(|(key, value)| Tag::new(key, value))
            .collect()
    }
}

// Serialized by hand rather than `#[serde(transparent)]`, which serde
// refuses to combine with the `try_from` that gates the way in.
impl Serialize for Tags {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// One tag spelled as `key=value`.
///
/// A tag takes this form where it crosses a boundary as a single string.
/// The parse validates it. A set of tags is a [`Tags`] map instead, where
/// a duplicate key is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Tag {
    key: String,
    value: String,
}

impl Tag {
    /// A `key: value` label, each part checked against its vocabulary.
    ///
    /// A key is `[A-Za-z0-9][A-Za-z0-9._/-]*`. The key excludes `=`, so
    /// `key=value` splits unambiguously wherever a tag crosses as one
    /// string. A value is any non-empty run of non-control characters.
    ///
    /// # Errors
    ///
    /// [`TagError`], naming the rule the input broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::Tag;
    ///
    /// assert!(Tag::new("session-id", "3f9c-1a").is_ok());
    /// assert!(Tag::new("has=equals", "v").is_err());
    /// ```
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Result<Tag, TagError> {
        let (key, value) = (key.into(), value.into());
        if key.is_empty() || value.is_empty() {
            return Err(TagError::Empty);
        }
        if !key.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return Err(TagError::KeyStart(key));
        }
        if let Some(c) = key.chars().find(|c| !is_key_char(*c)) {
            return Err(TagError::KeyChar(c));
        }
        if value.chars().any(char::is_control) {
            return Err(TagError::ValueControlChar(key));
        }
        Ok(Tag { key, value })
    }

    /// The tag's key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The tag's value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Why a string is not a [`Tag`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TagError {
    #[error("a tag is spelled key=value, and '{0}' carries no '='")]
    NotAPair(String),
    #[error("a tag key and value are both required")]
    Empty,
    #[error("a tag key starts alphanumeric, and '{0}' does not")]
    KeyStart(String),
    #[error("a tag key holds only [A-Za-z0-9._/-], and '{0}' is not one")]
    KeyChar(char),
    #[error("the value of tag '{0}' holds a control character")]
    ValueControlChar(String),
}

impl FromStr for Tag {
    type Err = TagError;

    /// Splits at the **first** `=`: a value may contain one, a key may not.
    ///
    /// # Errors
    ///
    /// [`TagError`], naming the rule the input broke.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nit_types::domain::Tag;
    ///
    /// let tag: Tag = "worktree=/w/a=b".parse().unwrap();
    /// assert_eq!((tag.key(), tag.value()), ("worktree", "/w/a=b"));
    /// assert!("branch".parse::<Tag>().is_err());
    /// ```
    fn from_str(arg: &str) -> Result<Tag, TagError> {
        let (key, value) = arg
            .split_once('=')
            .ok_or_else(|| TagError::NotAPair(arg.to_string()))?;
        Tag::new(key, value)
    }
}

impl TryFrom<String> for Tag {
    type Error = TagError;

    fn try_from(arg: String) -> Result<Tag, TagError> {
        arg.parse()
    }
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-')
}
