//! Wire types for nit's HTTP/JSON API.
//!
//! The contract for every shape that crosses the boundary, shared by the
//! axum server (`crates/nit`) and its CLI through one set of `serde`
//! derives and exported to the web as `web/src/api/types.gen.ts`
//! (`nix run .#gen-types`). The doc-comments here carry the semantics the
//! shapes alone cannot; route-level behavior (status codes, query params)
//! lives on the server's handlers.
//!
//! Conventions: every non-2xx response is the [`error`] envelope; times
//! are RFC3339 strings; shas are full 40-hex, truncated by clients to the
//! canonical 12-char short form for display.
//!
//! Dependency-light by construction — `serde` only (the `clap` derive on
//! `Side` is feature-gated off) and never `serde_json::Value` — so a future
//! web build can share these types without pulling in the server, and every
//! payload is a typed shape rather than dynamic JSON.

pub mod chain;
pub mod chains;
pub mod changes;
pub mod comments;
pub mod decisions;
pub mod diff;
pub mod domain;
pub mod error;
pub mod events;
pub mod fold;
pub mod graph;
pub mod health;
pub mod log;
pub mod push;
pub mod repos;

#[cfg(test)]
mod tests;

// The single-file TypeScript export of the web-facing wire types.
#[cfg(all(test, feature = "ts"))]
mod export;
