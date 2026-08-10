//! The shared closed vocabularies of nit.
//!
//! The small sets of named values — sides, verdicts, statuses, kinds —
//! used across the server's domain fold, the wire DTOs that live beside
//! them in this crate, and the CLI alike.
//!
//! **Discipline: a closed set of values is an `enum`, never a `String`.**
//! Every value that can only be one of a fixed list lives here as a serde
//! enum whose `rename`/`rename_all` fixes its on-the-wire spelling, so the
//! *same* type is the domain value, the JSON shape, and the parsed CLI
//! input. The payoff is concrete: an exhaustive `match` instead of a
//! `_ =>` fallthrough, no `as_str`/`from_str` round-tripping at the
//! domain↔wire boundary, and — because `#[serde(deny_unknown…)]`-style
//! rejection is automatic for enums — an unknown value is a clean
//! deserialization error (a 400 through `AppJson`), not a string that flows
//! deeper before something notices. New enumerated fields are added here and
//! referenced from both sides; they are never reintroduced as `String`.
//!
//! Serde renamings pin the exact wire spellings, so swapping a `String`
//! field for one of these enums is not a wire change.

mod conversation;
mod log;
mod rendering;
mod verdict;

pub use conversation::*;
pub use log::*;
pub use rendering::*;
pub use verdict::*;
