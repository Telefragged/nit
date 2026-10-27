//! The repository registry.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct Repo {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and display name.
    pub git_dir: String,
    /// The one canonical ref; mergedness tracks it.
    pub canonical_ref: String,
    /// How many changes are neither merged nor abandoned (derived, never
    /// stored).
    pub open_changes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(utoipa::ToSchema)]
pub struct RepoList {
    pub repos: Vec<Repo>,
}

/// `POST /api/repos` request — register a repo (`nit repo create`).
///
/// `canonical_ref` names the ref the repo tracks; it must resolve to a
/// commit — any git ref, e.g. `origin/main`.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateRepo {
    pub git_dir: String,
    pub canonical_ref: String,
}

/// `PATCH /api/repos/{id}` request (this is `nit repo move`).
///
/// Repoints a moved repo at its new git-common-dir.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RelocateRepo {
    pub git_dir: String,
}

/// `PUT /api/repos/{id}/canonical-ref` request (this is `nit repo set-ref`).
///
/// Points a repo at a new canonical ref. The new ref must resolve to the
/// canonical HEAD that nit last recorded for the repo. Thus no change has
/// merged on one ref but not on the other.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SetCanonicalRef {
    pub canonical_ref: String,
}
