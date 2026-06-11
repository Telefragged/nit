//! Git scan engine: reconciles a registered branch (`base..tip`) with the
//! review database — docs/data-model.md "Scan algorithm" is the contract.
//!
//! - [`fixup`] — `fixup!`/`squash!` classification and autosquash target
//!   attachment (pure logic, differentially tested against git).
//! - [`identity`] — `Change-Id:` trailer extraction and duplicate-trailer
//!   derived keys.

pub mod fixup;
pub mod identity;
