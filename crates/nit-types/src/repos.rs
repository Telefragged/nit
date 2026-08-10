//! The repository registry.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Repo {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and display name.
    pub git_dir: String,
    /// The one canonical ref; mergedness tracks it.
    pub canonical_ref: String,
    /// Live tip count (derived from the tip set, never stored).
    pub active_chains: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoList {
    pub repos: Vec<Repo>,
}

/// `POST /api/repos` request — register a repo (`nit repo create`).
///
/// `canonical_ref` names the ref the repo tracks; it must resolve to a
/// commit — any git ref, e.g. `origin/main`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepo {
    pub git_dir: String,
    pub canonical_ref: String,
}

/// `PATCH /api/repos/{id}` request (this is `nit repo move`).
///
/// Repoints a moved repo at its new git-common-dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelocateRepo {
    pub git_dir: String,
}
