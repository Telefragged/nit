//! HTTP API: every endpoint of `docs/api.md`, axum 0.8.
//!
//! - [`types`] — the wire-shape mirror of docs/api.md (golden rule 3).
//! - [`diff`] — diff JSON rendering and comment-anchor porting.

pub mod diff;
pub mod types;
