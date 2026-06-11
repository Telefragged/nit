//! nit — commit-level code review for AI coding agents.
//!
//! Library surface for the `nit` binary and its tests:
//!
//! - [`db`] — SQLite persistence: open/migrate, typed rows, query helpers
//!   (schema contract: `docs/data-model.md`).
//! - [`gitscan`] — the scan engine: walks `base..tip` of a registered
//!   branch, reconciles changes/revisions, folds `fixup!` commits, detects
//!   merged/abandoned chains (`docs/data-model.md` "Scan algorithm").

pub mod db;
pub mod gitscan;
