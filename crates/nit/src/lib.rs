//! nit — commit-level code review for AI coding agents.
//!
//! Library surface for the `nit` binary and its tests:
//!
//! - [`enums`] — the shared closed vocabularies (sides, verdicts, statuses,
//!   kinds): one serde enum per fixed value set, used by the domain, the
//!   wire, and the CLI alike (never a `String`).
//! - [`db`] — `SQLite` persistence: open/migrate, typed rows, query helpers
//!   (schema contract: `docs/data-model.md`).
//! - [`gitscan`] — the scan engine: walks `base..tip` of a registered
//!   branch, reconciles changes/revisions, detects merged/abandoned
//!   chains (`docs/data-model.md` "Scan algorithm").
//! - [`api`] — the axum HTTP layer (`docs/api.md` is the contract) plus
//!   the `nit serve` wiring.
//! - [`cli`] — `nit push`/`wait`/`status`/`reply`, thin clients of the API.

#![deny(clippy::unwrap_used)]

pub mod api;
pub mod cli;
pub mod db;
pub mod enums;
pub mod gitscan;
pub mod review;
