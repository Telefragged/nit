//! WebAssembly bindings for the shared change fold (`nit_types::fold`): the
//! browser folds the websocket stream client-side with the very same Rust code
//! the server runs. The server ships a `ChangeProj` snapshot; the browser
//! resumes folding the live tail onto it and projects the published
//! `ChangeDetail` — never reimplementing the fold.
//!
//! Values cross the boundary as structured `JsValue`s via `serde-wasm-bindgen`,
//! with no JSON text in between. `u64` rides as a JS `number` — the same
//! representation the web already holds — so the wire types are unchanged.

use nit_types::fold::{self, ChangeProj};
use nit_types::log::LogEntry;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Serialize a fold value to a `JsValue`. `serialize_missing_as_null` keeps
/// `Option::None` as JS `null` (the default is `undefined`), which the web's
/// `=== null` checks and the `… | null` wire types depend on.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true);
    Ok(value.serialize(&serializer)?)
}

/// A change's identity (not carried in the log) plus its log entries, ascending
/// by `idx` — the input to [`replay_proj`].
#[derive(Deserialize)]
struct ReplayInput {
    id: u64,
    repo_id: u64,
    change_key: String,
    entries: Vec<LogEntry>,
}

/// Fold a change's whole log into its `ChangeProj` snapshot. The mock builds
/// snapshots this way to mirror the server, which folds natively.
///
/// # Errors
/// When `input` is not a valid input or the projection fails to serialize.
#[wasm_bindgen]
pub fn replay_proj(input: JsValue) -> Result<JsValue, JsValue> {
    let input: ReplayInput = serde_wasm_bindgen::from_value(input)?;
    let proj = fold::replay(input.id, input.repo_id, input.change_key, input.entries);
    to_js(&proj)
}

/// Apply one live log entry to a `ChangeProj`, returning the advanced
/// projection. Idempotent across the snapshot/live overlap: an entry below the
/// snapshot's high-water mark is a no-op.
///
/// # Errors
/// When either argument fails to parse or the result fails to serialize.
#[wasm_bindgen]
pub fn fold_entry(proj: JsValue, entry: JsValue) -> Result<JsValue, JsValue> {
    let mut proj: ChangeProj = serde_wasm_bindgen::from_value(proj)?;
    let entry: LogEntry = serde_wasm_bindgen::from_value(entry)?;
    fold::fold(&mut proj, entry);
    to_js(&proj)
}

/// Project a `ChangeProj` to its published `ChangeDetail` (docs/api.md
/// "Changes") — revisions, threads, reviews. The reviewer's drafts and staged
/// decision are not log state, so they come back empty; the browser overlays
/// its own from `GET /changes/{id}/drafts`.
///
/// # Errors
/// When `proj` is not a valid projection or the result fails to serialize.
#[wasm_bindgen]
pub fn change_detail(proj: JsValue) -> Result<JsValue, JsValue> {
    let proj: ChangeProj = serde_wasm_bindgen::from_value(proj)?;
    to_js(&fold::change_detail(&proj))
}
