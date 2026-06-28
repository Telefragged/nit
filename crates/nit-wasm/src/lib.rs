//! WebAssembly bindings for the shared change fold (`nit_types::fold`): the
//! browser folds the websocket stream client-side with the very same Rust code
//! the server runs. The server ships a `ChangeProj` snapshot; the browser
//! resumes folding the live tail onto it and projects the published
//! `ChangeDetail` — never reimplementing the fold.
//!
//! JSON crosses the boundary as strings (not `u64`/`BigInt` args), so the web's
//! plain `number` ids round-trip through serde unchanged.

use nit_types::fold::{self, ChangeProj};
use nit_types::log::LogEntry;
use serde::Deserialize;
use wasm_bindgen::prelude::*;

/// A change's identity (not carried in the log) plus its log entries, ascending
/// by `idx` — the input to [`replay_proj`].
#[derive(Deserialize)]
struct ReplayInput {
    id: u64,
    repo_id: u64,
    change_key: String,
    entries: Vec<LogEntry>,
}

/// Fold a change's whole log into its `ChangeProj` snapshot JSON. The mock
/// builds snapshots this way to mirror the server, which folds natively.
///
/// # Errors
/// When `input_json` is not a valid input or the projection fails to serialize.
#[wasm_bindgen]
pub fn replay_proj(input_json: &str) -> Result<String, JsError> {
    let input: ReplayInput = serde_json::from_str(input_json)?;
    let proj = fold::replay(input.id, input.repo_id, input.change_key, input.entries);
    Ok(serde_json::to_string(&proj)?)
}

/// Apply one live log entry to a `ChangeProj`, returning the advanced
/// projection JSON. Idempotent across the snapshot/live overlap: an entry below
/// the snapshot's high-water mark is a no-op.
///
/// # Errors
/// When either argument fails to parse or the result fails to serialize.
#[wasm_bindgen]
pub fn fold_entry(proj_json: &str, entry_json: &str) -> Result<String, JsError> {
    let mut proj: ChangeProj = serde_json::from_str(proj_json)?;
    let entry: LogEntry = serde_json::from_str(entry_json)?;
    fold::fold(&mut proj, entry);
    Ok(serde_json::to_string(&proj)?)
}

/// Project a `ChangeProj` to its published `ChangeDetail` JSON (docs/api.md
/// "Changes") — revisions, threads, reviews. The reviewer's drafts and staged
/// decision are not log state, so they come back empty; the browser overlays
/// its own from `GET /changes/{id}/drafts`.
///
/// # Errors
/// When `proj_json` is not a valid projection or the result fails to serialize.
#[wasm_bindgen]
pub fn change_detail(proj_json: &str) -> Result<String, JsError> {
    let proj: ChangeProj = serde_json::from_str(proj_json)?;
    Ok(serde_json::to_string(&fold::change_detail(&proj))?)
}
