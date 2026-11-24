//! The shapes of a document on the HTTP API and on its websocket.

use std::collections::BTreeMap;

use nit_types::domain::{Tag, Tags};
use serde::{Deserialize, Serialize};

use crate::{Update, Version};

/// The number of a document on a server.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DocumentNumber(u64);

impl DocumentNumber {
    #[must_use]
    pub const fn new(number: u64) -> DocumentNumber {
        DocumentNumber(number)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for DocumentNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for DocumentNumber {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<DocumentNumber, std::num::ParseIntError> {
        s.parse().map(DocumentNumber)
    }
}

/// The `POST /api/documents` request: an empty document.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CreateDocument {
    pub title: String,
    /// The tags of the document. They never change.
    #[serde(default)]
    pub tags: Tags,
}

/// A document without its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DocumentSummary {
    pub number: DocumentNumber,
    pub title: String,
    /// The edits that the server has.
    #[schema(value_type = String)]
    pub version: Version,
    /// The version that a reviewer last sent to the author.
    #[schema(value_type = Option<String>)]
    pub sent: Option<Version>,
}

/// The `GET /api/documents` response.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DocumentList {
    pub documents: Vec<DocumentSummary>,
}

/// Selects documents. With no tag, the query selects every document.
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DocumentQuery {
    /// Documents that have every one of these tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
    pub tag: Vec<Tag>,
}

/// What a client of `WS /api/documents/stream` follows. The client
/// sends it as a text frame, and a later subscription replaces it.
///
/// The server sends the edits that the client lacks as a binary frame
/// ([`update_frame`]), and each sent version as a [`DocumentSent`]
/// text frame.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DocumentSubscription {
    /// The documents to receive edits of, each with the version that
    /// the client has.
    #[serde(default)]
    pub updates: BTreeMap<DocumentNumber, Version>,
    /// The documents to receive the sent versions of.
    #[serde(default)]
    pub sent: Option<DocumentQuery>,
    /// The sent version of each document that the client has seen.
    #[serde(default)]
    pub seen: BTreeMap<DocumentNumber, Version>,
}

/// A text frame from `WS /api/documents/stream`: a reviewer sent
/// this version of the document to the author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DocumentSent {
    pub document: DocumentNumber,
    pub version: Version,
}

/// A binary frame from `WS /api/documents/stream`: the number of the
/// document as 8 bytes, little-endian, then the bytes of the update.
#[must_use]
pub fn update_frame(document: DocumentNumber, update: &Update) -> Vec<u8> {
    let mut frame = document.get().to_le_bytes().to_vec();
    frame.extend_from_slice(update.as_bytes());
    frame
}
