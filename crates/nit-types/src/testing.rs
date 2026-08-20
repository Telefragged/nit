//! Fixture values that satisfy the domain types' constructors.
//!
//! A test names a commit `A1` or `base` and a change `Iabc`. Git and the
//! `Change-Id` trailer are spelled in 40 hex characters, so a fixture
//! expands its label to reach them.

use crate::domain::{ChangeId, Sha};

/// A distinct, well-formed object name for `label`.
///
/// # Panics
///
/// Never: the expansion is 40 hex characters by construction.
#[must_use]
pub fn sha(label: &str) -> Sha {
    Sha::new(hex_expansion(label)).expect("a hex expansion of the label")
}

/// A distinct, well-formed change identity for `label`.
///
/// # Panics
///
/// Never: the expansion is `I` and 40 hex characters by construction.
#[must_use]
pub fn change_id(label: &str) -> ChangeId {
    let body = hex_expansion(label.strip_prefix('I').unwrap_or(label));
    ChangeId::new(format!("I{body}")).expect("a hex expansion of the label")
}

/// `label` as 40 hex characters: itself when it is already hex, its bytes
/// otherwise, zero-padded either way.
fn hex_expansion(label: &str) -> String {
    let hex: String = if label.chars().all(|c| c.is_ascii_hexdigit()) {
        label.to_string()
    } else {
        label.bytes().fold(String::new(), |mut hex, b| {
            use std::fmt::Write;
            let _ = write!(hex, "{b:02x}");
            hex
        })
    };
    format!("{hex:0<40}")[..40].to_string()
}
