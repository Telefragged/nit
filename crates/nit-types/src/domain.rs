//! nit's domain model: the vocabulary every component speaks.
//!
//! A term is defined once, on the type that carries it, and that type
//! lives here — so this page is the model in full. A term with no type of
//! its own is defined in the prose below: roles, acts, and the relations
//! between terms.
//!
//! A shape that exists to serve a route — a request body, a list
//! envelope, the published rendering of a term defined here — belongs to
//! the module behind that route and points back here for what its fields
//! mean.
//!
//! A definition says what a term **is**. Anything a code change could
//! falsify — a count, a route, a column, a status code, a behavior —
//! belongs to the code it describes and never to a definition.
//!
//! # Who
//!
//! **Author** — the party that pushes changes and answers review.
//!
//! **Reviewer** — the party that reads a change and returns a verdict.
//!
//! # What is reviewed
//!
//! **Change** — one commit under review, and the thing every verdict,
//! comment and status attaches to. Its identity is its `Change-Id`.
//!
//! **Revision** — one version of a change.
//!
//! **Chain** — a change and the changes stacked on it, walked from the
//! tip back to the canonical ref.
//!
//! # The conversation
//!
//! **Thread** — a located, resolvable conversation, anchored to a change,
//! a file, or a line of a revision.
//!
//! **Comment** — one message in a thread. A reviewer's is published by a
//! review; an author's stands on its own.
//!
//! # What happens
//!
//! **Push** — an author offering the current tip for review. Nothing is
//! reviewable until it is pushed.
//!
//! **Amend** — rewriting a reviewed commit so a fix lands in the change
//! that drew it rather than in a change stacked on top.
//!
//! **Interdiff** — the diff between two revisions of one change. It is
//! how a reviewer reads an amend.
//!
//! **Merge** — putting an approved change onto the canonical ref.
//!
//! # Where things sit
//!
//! **Canonical ref** — the one ref a repo tracks. It is the base a chain
//! forks from and the yardstick for whether a change has merged.
//!
//! **Fork point** — the commit on the canonical ref that a revision was
//! pushed against.
//!
//! **Tip** — the newest change of a chain, and the commit a push walks
//! back from.
//!
//! # How current state is arrived at
//!
//! **Log** — a change's append-only record: one [`LogEntry`] per thing
//! that happened to it, in the order it happened.
//!
//! **Folding** — replaying a change's log entries in order to arrive at
//! what is true of it now. **Projection** — what a fold produces
//! ([`ChangeProjection`]); the algorithm is `crate::fold`.
//!
//! # Verdict, decision, status, state
//!
//! Four terms a reader will otherwise conflate:
//!
//! - [`Verdict`] — what a reviewer concluded about a change.
//! - [`Decision`] — a conclusion the reviewer has drafted privately and
//!   not yet published. A superset of the verdicts, because abandoning
//!   is chosen in the same breath as approving.
//! - [`ChangeStatus`] — where one change stands, at one revision.
//! - [`ChainState`] — whose turn it is on a chain.
//!
//! A reviewer chooses the first two. The last two are derived, and
//! nobody sets them by hand.
//!
//! # Naming
//!
//! **Terms are spelled out.** An identifier carries a domain term in
//! full, so that the term's definition is the only thing a reader needs
//! to understand the name. The exemptions are a closed set — `id`,
//! `sha`, `repo`, `git_dir` — and adding to it is a decision, not a
//! convenience.
//!
//! **A closed set of values is an `enum`, never a `String`.**
//! Every value that can only be one of a fixed list lives here as a serde
//! enum whose `rename`/`rename_all` fixes its on-the-wire spelling, so the
//! *same* type is the domain value, the JSON shape, and the parsed CLI
//! input. The payoff is concrete: an exhaustive `match` instead of a
//! `_ =>` fallthrough, no `as_str`/`from_str` round-tripping at the
//! domain↔wire boundary, and — because `#[serde(deny_unknown…)]`-style
//! rejection is automatic for enums — an unknown value is a clean
//! deserialization error (a 400 through `AppJson`), not a string that flows
//! deeper before something notices. New enumerated fields are added here and
//! referenced from both sides; they are never `String`.
//!
//! Serde renamings pin the exact wire spellings, so swapping a `String`
//! field for one of these enums is not a wire change.

mod chain;
mod change;
mod conversation;
mod identity;
mod log;
mod rendering;
mod verdict;

pub use chain::*;
pub use change::*;
pub use conversation::*;
pub use identity::*;
pub use log::*;
pub use rendering::*;
pub use verdict::*;
