// The shared change fold, compiled to WebAssembly (crates/nit-wasm). The change
// page folds the websocket stream client-side with the same Rust code the
// server runs: the server ships a ChangeProjection, the browser resumes
// folding the live tail onto it and projects the published ChangeDetail — never
// reimplementing the projection.
//
// serde-wasm-bindgen moves these values across the boundary as structured
// objects, with no JSON text in between; `u64` rides as a plain `number`, the
// representation the web already holds. A ChangeProjection is opaque to the web —
// only these wrappers (re)hydrate it.

import {
  change_detail,
  fold_entry,
  repo_graph,
  replay_proj,
} from "../wasm/nit_wasm";
import type {
  ChangeDetail,
  ChangeProjection,
  LogEntry,
  RepoGraph,
  RepoHistory,
} from "./types";

/** A change's identity (not carried in the log) plus its entries, ascending by
 * `position` — the input to {@link replayProjection}. */
export interface ReplayInput {
  id: number;
  repo_id: number;
  change_id: string;
  entries: LogEntry[];
}

/** Fold a change's whole log into its `ChangeProjection` — what the mock
 * ships to mirror the server, which folds natively. */
export function replayProjection(input: ReplayInput): ChangeProjection {
  return replay_proj(input) as ChangeProjection;
}

/** Idempotent across the projection/live overlap (an entry below the
 * projection's high-water mark is a no-op). */
export function foldEntry(
  proj: ChangeProjection,
  entry: LogEntry,
): ChangeProjection {
  return fold_entry(proj, entry) as ChangeProjection;
}

/** Published projection only — drafts and the draft decision are not log
 * state, so they come back empty; the caller overlays its own
 * (`GET /changes/{id}/drafts`). */
export function changeDetail(proj: ChangeProjection): ChangeDetail {
  return change_detail(proj) as ChangeDetail;
}

/** Assemble the repo's change graph from its change folds
 * (`GET /api/changes`) and canonical history (`GET /api/history`). */
export function repoGraph(
  changes: ChangeProjection[],
  history: RepoHistory,
): RepoGraph {
  return repo_graph(changes, history) as RepoGraph;
}
