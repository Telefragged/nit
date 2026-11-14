//! nit's domain model: the vocabulary that every component uses.
//!
//! A term is defined once, on the type that represents it, and that type
//! is in this module, so this page is the whole model. A term with no type
//! of its own is defined in the prose below: roles, acts, and the
//! relations between terms.
//!
//! A shape that exists to serve a route — a request body, a list
//! envelope, the published rendering of a term defined here — belongs to
//! the module behind that route and links back here for the definitions
//! of its fields.
//!
//! A definition documents what a term **is**. Anything a code change could
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
//! **Change version** — a revision, or the base of a revision: the parent
//! commit of the revision, with the attachments put on that base. The diff
//! of a revision starts from its base ([`ChangeVersion`]).
//!
//! **Attachment** — a file stored on a change version beside its commit,
//! under a name that is a relative path. Its bytes are identified by
//! their hash, so the same bytes can be on many versions. A later revision
//! shows the attachments of an earlier one too, and the base of a later
//! revision those of an earlier base ([`EffectiveAttachment`]).
//!
//! **Tag** — a `key: value` label set on a change. A `tags` entry
//! stands on its own, so a key keeps its value until a later entry sets
//! it again and labelling costs no revision. Tags correlate changes that
//! git ancestry does not.
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
//! **Ported comment** — a thread shown at a revision it was not written
//! on, at the place its anchor maps to in that revision's trees.
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
//! **Canonical ref** — the one ref a repo tracks. It is the base a push
//! forks from and the ref that determines whether a change has merged.
//!
//! **Fork point** — the commit on the canonical ref that a revision was
//! pushed against.
//!
//! **Tip** — a change's latest revision that no revision records as its
//! parent, and the commit a push walks back from.
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
//! # Verdict, decision, status
//!
//! Three terms a reader will otherwise conflate:
//!
//! - [`Verdict`] — what a reviewer concluded about a change.
//! - [`Decision`] — a conclusion the reviewer has drafted privately and
//!   not yet published. A superset of the verdicts, because a reviewer
//!   chooses between abandoning and approving in one step.
//! - [`ChangeStatus`] — where one change stands, at one revision.
//!
//! A reviewer chooses the first two. The last is derived, and nobody
//! sets it by hand.

mod attachment;
mod change;
mod conversation;
mod identity;
mod log;
mod porting;
mod rendering;
mod tag;
mod verdict;

pub use attachment::*;
pub use change::*;
pub use conversation::*;
pub use identity::*;
pub use log::*;
pub use porting::*;
pub use rendering::*;
pub use tag::*;
pub use verdict::*;
