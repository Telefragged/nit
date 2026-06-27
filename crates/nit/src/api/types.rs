//! The server's view of the API wire types: per-domain shapes defined once in
//! `nit-types` (the single source of truth, shared with the CLI) and surfaced
//! here as stable `types::*` paths. Enumerated values are the shared serde
//! enums of [`crate::enums`]. Change `docs/api.md` first, then the `nit-types`
//! definition.

use serde::{Deserialize, Serialize};

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
pub use nit_types::log::{ChainLog, LogEntry, LogPayload};

// ---------------------------------------------------------------------------
// Events (WS /api/stream) — docs/api.md "Events"

/// A server → client websocket message: a tagged [`LogEntry`], or the
/// out-of-log `new_parent` advisory. `untagged` so an entry serializes as its
/// bare fields and `new_parent` as `{"new_parent": {…}}` — the client tells
/// them apart by the `new_parent` key.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum StreamMsg {
    Entry(LogEntry),
    NewParent { new_parent: NewParent },
}

#[derive(Debug, Clone, Serialize)]
pub struct NewParent {
    /// The child end of the newly established edge (a re-rooted change, or a
    /// brand-new child stacked on `parent`).
    pub of: u64,
    /// The parent `of` now sits on.
    pub parent: u64,
}

/// A client → server websocket message: subscribe a set of changes, each from
/// an idx. Externally tagged. The map keys are **strings** — serde cannot
/// deserialize integer map keys through the content-buffering an
/// internally-tagged/untagged enum needs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMsg {
    Subscribe(std::collections::HashMap<String, u64>),
}
