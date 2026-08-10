//! The Rust the browser runs: the change fold and the intraline diff.
//!
//! The fold is the shared one (`nit_types::fold`), so the websocket stream
//! folds client-side with the very same code the server runs — the server
//! ships a `ChangeProjection`, the browser resumes folding the live tail
//! onto it and projects the published `ChangeDetail`, never reimplementing
//! the fold. The intraline diff's per-character Myers wants a compiled
//! language.
//!
//! Values cross the boundary as structured `JsValue`s via `serde-wasm-bindgen`,
//! with no JSON text in between. `u64` rides as a JS `number` — the same
//! representation the web already holds — so the wire types are unchanged.

use nit_types::chain::RepoView;
use nit_types::domain::ChangeNumber;
use nit_types::domain::ChangeProjection;
use nit_types::fold;
use nit_types::graph::RepoHistory;
use nit_types::log::LogEntry;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

pub mod graph;

mod intraline;

/// Serializes a fold value to a `JsValue`.
///
/// `serialize_missing_as_null` keeps `Option::None` as JS `null` (the
/// default is `undefined`), which the web's `=== null` checks and the
/// `… | null` wire types depend on.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true);
    Ok(value.serialize(&serializer)?)
}

/// The input to [`replay_proj`].
///
/// A change's identity (not carried in the log) plus its log entries,
/// ascending by `position`.
#[derive(Deserialize)]
struct ReplayInput {
    id: u64,
    repo_id: u64,
    change_key: String,
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
        ChangeNumber(input.id),
        input.repo_id,
        input.change_key.into(),
        input.entries,
    );
    to_js(&proj)
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
/// history (`GET /api/history`).
///
/// # Errors
///
/// When either argument fails to parse or the graph fails to serialize.
#[wasm_bindgen]
pub fn repo_graph(changes: JsValue, history: JsValue) -> Result<JsValue, JsValue> {
    let changes: Vec<ChangeProjection> = serde_wasm_bindgen::from_value(changes)?;
    let history: RepoHistory = serde_wasm_bindgen::from_value(history)?;
    to_js(&graph::assemble(&RepoView::new(changes), &history))
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

/// Projects a `ChangeProjection` to its published `ChangeDetail`.
///
/// The detail carries revisions, threads and reviews. The reviewer's drafts
/// and draft decision are not log state, so they come back empty; the
/// browser overlays its own from `GET /changes/{id}/drafts`.
///
/// # Errors
///
/// When `proj` is not a valid projection or the result fails to serialize.
#[wasm_bindgen]
pub fn change_detail(proj: JsValue) -> Result<JsValue, JsValue> {
    let proj: ChangeProjection = serde_wasm_bindgen::from_value(proj)?;
    to_js(&fold::change_detail(&proj))
}
