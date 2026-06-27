//! nit — commit-level code review for AI coding agents.
//!
//! Library surface for the `nit` binary and its tests:
//!
//! - [`enums`] — the shared closed vocabularies (sides, verdicts, statuses,
//!   kinds): one serde enum per fixed value set, used by the domain, the
//!   wire, and the CLI alike (never a `String`).
//! - [`db`] — `SQLite` persistence: open/migrate, typed rows, query helpers
//!   (schema contract: `docs/data-model.md`).
//! - [`review`] — the per-change fold: a change's reviewable state is the
//!   replay of its append-only log.
//! - [`chain`] — chain derivation: walks a tip's `parent_sha` back to the
//!   canonical base, a pure function of the per-change folds.
//! - [`gitscan`] — the git layer: the push walk, merged/abandoned detection,
//!   and GC-safety keep refs (`docs/data-model.md`).
//! - [`api`] — the axum HTTP layer (`docs/api.md` is the contract) plus
//!   the `nit serve` wiring.
//! - [`cli`] — `nit push`/`status`/`log`/`comment`, thin clients of the API.

#![deny(clippy::unwrap_used)]

/// The build's version: the crate semver plus `+<sha>[.dirty]` build metadata
/// when built from a git tree (`build.rs` sets `NIT_GIT_SUFFIX`, empty for a
/// tarball build). The single source for `nit --version` and `/api/health`.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), env!("NIT_GIT_SUFFIX"));

pub mod api;
pub mod chain;
pub mod cli;
pub mod db;
pub mod gitscan;
pub mod review;

/// The shared closed vocabularies (sides, verdicts, statuses, kinds), defined
/// once in `nit-types` and re-exported so `crate::enums::*` stays stable.
pub use nit_types::enums;
