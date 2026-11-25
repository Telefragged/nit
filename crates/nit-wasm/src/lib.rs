//! The Rust the browser runs: the change fold and the intraline diff.
//!
//! The fold is the shared one (`nit_types::fold`), so the websocket stream
//! folds client-side with the same code the server runs: the server ships a
//! `ChangeProjection`, and the browser resumes folding the live tail onto it
//! and projects the published `ChangeDetail`. The intraline diff runs a
//! per-character Myers diff, which requires a compiled language.
//!
//! Values cross the boundary as structured `JsValue`s via `serde-wasm-bindgen`.
//! A `u64` crosses as a JS `number`, the type the generated wire types use.

use nit_types::domain::Anchor;
use nit_types::domain::ChangeId;
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::domain::LogAnchor;
use nit_types::domain::LogEntry;
use nit_types::domain::RevisionNumber;
use nit_types::fold;
use nit_types::graph::RepoHistory;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

pub mod document;
pub mod graph;

mod intraline;

/// Serializes a fold value to a `JsValue`.
///
/// `serialize_missing_as_null` keeps `Option::None` as JS `null` (the
/// default is `undefined`), which the web's `=== null` checks and the
/// `… | null` wire types depend on. `serialize_maps_as_objects` gives a
/// map the shape the server's JSON gives it (`Tags` is the one that
/// crosses), so a value the fold built and a value the REST routes
/// returned have the same shape.
pub(crate) fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_missing_as_null(true)
        .serialize_maps_as_objects(true);
    Ok(value.serialize(&serializer)?)
}

/// The input to [`replay_proj`].
///
/// A change's identity plus its log entries, ascending by `position`.
#[derive(Deserialize)]
struct ReplayInput {
    id: u64,
    repo_id: u64,
    change_id: ChangeId,
    entries: Vec<LogEntry>,
}

/// Folds a change's whole log into its `ChangeProjection`.
///
/// The mock builds projections this way to mirror the server, which folds
/// natively.
///
/// # Errors
///
/// When `input` is not a valid input or the projection fails to serialize.
#[wasm_bindgen]
pub fn replay_proj(input: JsValue) -> Result<JsValue, JsValue> {
    let input: ReplayInput = serde_wasm_bindgen::from_value(input)?;
    let proj = fold::replay(
        ChangeNumber::new(input.id),
        input.repo_id,
        input.change_id,
        input.entries,
    );
    to_js(&proj)
}

#[derive(Serialize, Deserialize)]
struct LoggedAnchor {
    revision: RevisionNumber,
    anchor: LogAnchor,
}

/// Returns the anchor of a comment that the log records on a revision.
///
/// # Errors
///
/// When `logged` is not a revision and a log anchor, or the anchor fails
/// to serialize.
#[wasm_bindgen]
pub fn anchor_from_log(logged: JsValue) -> Result<JsValue, JsValue> {
    let logged: LoggedAnchor = serde_wasm_bindgen::from_value(logged)?;
    to_js(&Anchor::from_log(logged.revision, logged.anchor))
}

/// Returns the revision and the anchor that the log records for `anchor`.
///
/// # Errors
///
/// When `anchor` is not an anchor, or the result fails to serialize.
#[wasm_bindgen]
pub fn anchor_into_log(anchor: JsValue) -> Result<JsValue, JsValue> {
    let anchor: Anchor = serde_wasm_bindgen::from_value(anchor)?;
    let (revision, anchor) = anchor.into_log();
    to_js(&LoggedAnchor { revision, anchor })
}

/// Applies one live log entry to a `ChangeProjection`.
///
/// Returns the advanced projection. Idempotent across the projection/live
/// overlap: an entry below the projection's high-water mark is a no-op.
///
/// # Errors
///
/// When either argument fails to parse or the result fails to serialize.
#[wasm_bindgen]
pub fn fold_entry(proj: JsValue, entry: JsValue) -> Result<JsValue, JsValue> {
    let mut proj: ChangeProjection = serde_wasm_bindgen::from_value(proj)?;
    let entry: LogEntry = serde_wasm_bindgen::from_value(entry)?;
    fold::fold(&mut proj, entry);
    to_js(&proj)
}

/// Assembles the repo's change graph from the two primitive reads.
///
/// The reads are the change folds (`GET /api/changes`) and the canonical
/// history (`GET /api/history`). `group_by` specifies the tag key to group
/// the open region on.
///
/// # Errors
///
/// When either argument fails to parse or the graph fails to serialize.
#[wasm_bindgen]
#[expect(
    clippy::needless_pass_by_value,
    reason = "wasm-bindgen exports take owned values"
)]
pub fn repo_graph(
    changes: JsValue,
    history: JsValue,
    group_by: Option<String>,
) -> Result<JsValue, JsValue> {
    let changes: Vec<ChangeProjection> = serde_wasm_bindgen::from_value(changes)?;
    let history: RepoHistory = serde_wasm_bindgen::from_value(history)?;
    to_js(&graph::assemble(&changes, &history, group_by.as_deref()))
}

/// Assembles the tag graph: the change folds at their latest revisions.
///
/// # Errors
///
/// When `changes` fails to parse or the graph fails to serialize.
#[wasm_bindgen]
pub fn tag_graph(changes: JsValue) -> Result<JsValue, JsValue> {
    let changes: Vec<ChangeProjection> = serde_wasm_bindgen::from_value(changes)?;
    to_js(&graph::assemble_tag(&changes))
}

/// How many of the change's threads are unresolved, over every revision.
///
/// # Errors
///
/// When `proj` fails to parse.
#[wasm_bindgen]
pub fn unresolved_threads(proj: JsValue) -> Result<usize, JsValue> {
    let proj: ChangeProjection = serde_wasm_bindgen::from_value(proj)?;
    Ok(proj.unresolved())
}

/// Marks the characters that changed inside a diff's replacement blocks.
///
/// One list of ranges per line of the block (rationale and budgets in
/// `intraline`).
///
/// # Errors
///
/// When `regions` is not a list of replacement blocks or the marks fail to
/// serialize.
#[wasm_bindgen]
pub fn intraline_marks(regions: JsValue) -> Result<JsValue, JsValue> {
    let regions: Vec<intraline::Region> = serde_wasm_bindgen::from_value(regions)?;
    to_js(&intraline::marks(&regions))
}

/// The replacements that change the text `old` into the text `new`, in
/// order: each an `old` span and the `new` span that replaces it, as
/// `[start, end]` in UTF-16 code units with the end exclusive.
///
/// # Errors
///
/// When the replacements fail to serialize.
#[wasm_bindgen]
pub fn text_replacements(old: &str, new: &str) -> Result<JsValue, JsValue> {
    to_js(&intraline::replacements(old, new))
}

/// Projects a `ChangeProjection` to its published `ChangeDetail`.
///
/// The reviewer's drafts and draft decision are not log state, so they
/// come back empty; the browser overlays its own from
/// `GET /changes/{id}/drafts`.
///
/// # Errors
///
/// When `proj` is not a valid projection or the result fails to serialize.
#[wasm_bindgen]
pub fn change_detail(proj: JsValue) -> Result<JsValue, JsValue> {
    let proj: ChangeProjection = serde_wasm_bindgen::from_value(proj)?;
    to_js(&fold::change_detail(&proj))
}
