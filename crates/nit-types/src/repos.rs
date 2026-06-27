//! The repository registry (docs/api.md "Repos").

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub id: u64,
    /// Canonical git-common-dir — the repo's identity and display name.
    pub git_dir: String,
    /// The one canonical base ref; mergedness tracks it.
    pub base_ref: String,
    /// Live tip count (derived from the tip set, never stored).
    pub active_chains: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoList {
    pub repos: Vec<Repo>,
}

/// `POST /api/repos` request — register a repo (`nit repo create`). `base`
/// configures the one canonical base ref; it must resolve to a commit — any
/// git ref, e.g. `origin/main`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepo {
    pub git_dir: String,
    pub base: String,
}

/// `PATCH /api/repos/{id}` request — repoint a moved repo at its new
/// git-common-dir (`nit repo move`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelocateRepo {
    pub git_dir: String,
}
