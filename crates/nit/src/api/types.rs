//! The server's view of the API wire types: per-domain shapes defined once in
//! `nit-types` (the single source of truth, shared with the CLI) and surfaced
//! here as stable `types::*` paths. Enumerated values are the shared serde
//! enums of [`crate::enums`]. Change `docs/api.md` first, then the `nit-types`
//! definition.

pub use crate::enums::{
    ChainState, ChangeStatus, Decision, FileStatus, GraphSection, LineKind, LogKind, Side, Verdict,
};

pub use nit_types::chains::{Chain, ChainList, PathEntry};
pub use nit_types::changes::{AbandonRequest, ChangeDetail, Review, Revision, StagedDecision};
pub use nit_types::comments::{
    CommentRange, Draft, EditDraft, NewComment, NewDraft, Thread, ThreadComment,
};
pub use nit_types::decisions::{BatchSubmitResult, SubmitError};
pub use nit_types::diff::{Diff, DiffFile, Hunk, Line};
pub use nit_types::error::ApiError;
pub use nit_types::graph::{GraphNode, RepoGraph};
pub use nit_types::health::Health;
pub use nit_types::push::{PushRequest, PushResult, TipChange};
pub use nit_types::repos::{CreateRepo, RelocateRepo, Repo, RepoList};

// ---------------------------------------------------------------------------
// Agent endpoints (the log)
pub use nit_types::log::{
    ChainLog, CommentInput, LifecyclePayload, LogEntry, LogPayload, ReviewPayload, RevisionPayload,
};

// ---------------------------------------------------------------------------
// Events (WS /api/stream) — docs/api.md "Events"
pub use nit_types::events::{ClientMsg, NewParent, StreamMsg};
