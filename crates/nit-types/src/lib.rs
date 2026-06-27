//! Wire types for nit's HTTP/JSON API — the single source of truth for every
//! shape that crosses the boundary, shared by the axum server (`crates/nit`)
//! and its CLI through one set of `serde` derives. `docs/api.md` is the prose
//! contract; this crate is its Rust form, organized into one module per
//! `docs/api/*` section.
//!
//! Dependency-light by construction — `serde` only (the `clap` derive on
//! `Side` is feature-gated off) and never `serde_json::Value` — so a future
//! web build can share these types without pulling in the server, and every
//! payload is a typed shape rather than dynamic JSON.

pub mod comments;
pub mod enums;
