//! The nit document.
//!
//! **Document** — one Markdown text that embeds references to changes.
//! Authors and reviewers edit one document at the same time, each on a
//! replica, and exchange [`Update`]s. The replicas converge. Documents
//! belong to no change, and no change log records them.
//!
//! **Reference** — a link on a line of its own, to a part of a change or
//! to a file uploaded to the document ([`Reference`]). The document page
//! shows it as that part. References point at changes. No change points
//! at a document.
//!
//! **Writer** — the side that made an edit ([`Writer`]).
//!
//! **Edit** — the lines that differ between two versions of the text,
//! with the writers that changed them ([`Edit`]).

mod document;
mod reference;
#[cfg(test)]
mod tests;

pub use document::{
    Document, Edit, Editor, LoadError, MergeError, ReplaceError, SpliceError, UnknownVersion,
    Update, Version, VersionError, Writer,
};
pub use reference::{
    LineRange, LineRangeError, PlacedReference, Reference, ReferenceError, references,
};
