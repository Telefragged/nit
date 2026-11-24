//! A reviewer's replica of a document, for the browser.
//!
//! The browser runs the same `nit_document` code as the server and the
//! CLI, so the three agree on every merge.

use nit_document::{Document, Reference, Update, Version, Writer};
use wasm_bindgen::prelude::*;

use crate::to_js;

/// One replica of a document, with `Writer::Reviewer` as its writer.
#[wasm_bindgen]
pub struct DocumentReplica {
    document: Document,
}

fn error(error: &impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

#[wasm_bindgen]
impl DocumentReplica {
    /// A replica of the document in `snapshot`, the body of
    /// `GET /api/documents/{number}/snapshot`.
    ///
    /// # Errors
    ///
    /// When the bytes are not a snapshot.
    #[wasm_bindgen(constructor)]
    pub fn new(snapshot: &[u8]) -> Result<DocumentReplica, JsError> {
        let document = Document::load(snapshot).map_err(|e| error(&e))?;
        Ok(DocumentReplica { document })
    }

    /// The Markdown text of the document.
    #[must_use]
    pub fn text(&self) -> String {
        self.document.text()
    }

    /// The `Version` of this replica.
    #[must_use]
    pub fn version(&self) -> String {
        self.document.version().to_string()
    }

    /// The `Run[]` of the document: its versions, the oldest first.
    ///
    /// # Errors
    ///
    /// When the runs fail to serialize.
    pub fn runs(&self) -> Result<JsValue, JsValue> {
        to_js(&self.document.runs())
    }

    /// The `Line[]` of a diff from the text at version `from` to the
    /// text at version `to`: every line of both texts, as the lines
    /// route of a file returns them.
    ///
    /// # Errors
    ///
    /// When `from` or `to` is not a version of this replica.
    pub fn lines_between(&self, from: &str, to: &str) -> Result<JsValue, JsValue> {
        let version = |version: &str| version.parse::<Version>().map_err(|e| error(&e));
        let lines = self
            .document
            .lines_between(&version(from)?, &version(to)?)
            .map_err(|e| error(&e))?;
        to_js(&lines)
    }

    /// Replaces the whole text with `text`, by one splice of the
    /// characters that differ. Equal texts make no edit.
    pub fn set_text(&mut self, text: &str) {
        self.document.editor(Writer::Reviewer).set_text(text);
    }

    /// Adds `text` at the end as a block of its own, after one empty
    /// line.
    pub fn append(&mut self, text: &str) {
        self.document.editor(Writer::Reviewer).append(text);
    }

    /// The bytes of the edits that this replica has and a replica at
    /// `version` lacks: the body of `POST /api/documents/{number}/updates`.
    ///
    /// # Errors
    ///
    /// When `version` is not a version.
    pub fn updates_since(&self, version: &str) -> Result<Vec<u8>, JsError> {
        let version: Version = version.parse().map_err(|e| error(&e))?;
        Ok(self.document.updates_since(&version).as_bytes().to_vec())
    }

    /// Takes in the bytes of an update from the document stream.
    ///
    /// # Errors
    ///
    /// When the bytes are not an update.
    pub fn merge(&mut self, update: Vec<u8>) -> Result<(), JsError> {
        self.document
            .merge(&Update::from_bytes(update))
            .map_err(|e| error(&e))
    }
}

/// The `PlacedReference[]` of the Markdown `text`: each reference with
/// the first line of its paragraph.
///
/// # Errors
///
/// When the references fail to serialize.
#[wasm_bindgen]
pub fn references(text: &str) -> Result<JsValue, JsValue> {
    to_js(&nit_document::references(text))
}

/// The address of a `Reference`.
///
/// # Errors
///
/// When `reference` is not a reference.
#[wasm_bindgen]
pub fn reference_address(reference: JsValue) -> Result<String, JsValue> {
    let reference: Reference = serde_wasm_bindgen::from_value(reference)?;
    Ok(reference.to_string())
}
