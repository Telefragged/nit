//! nit — commit-level code review for AI coding agents.
//!
//! Library surface for the `nit` binary and its tests:
//!
//! - [`db`] — SQLite persistence: open/migrate, typed rows, query helpers
//!   (schema contract: `docs/data-model.md`).

pub mod db;
