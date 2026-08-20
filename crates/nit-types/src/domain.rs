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
//! How a type here is built — validation at construction, illegal
//! states made unspellable, the serde gate, how a term is named — is
//! the `domain-modeling` skill.
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
//! **Draft** — a comment or a decision the reviewer has written but not
//! published; private to them until a review publishes it.
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
//! **Repo** — a git repository nit reviews for, identified by its
//! git-common-dir.
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
