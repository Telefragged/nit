//! Local git helpers: discover the cwd's repo, resolve its `git-common-dir`,
//! read HEAD, and resolve an explicit rev to a full commit sha.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use git2::Repository;

pub(crate) fn repo_git_dir(path: &Path) -> Result<String> {
    let repo = Repository::discover(path).map_err(|e| {
        anyhow!(
            "not a git repository at {}: {}",
            path.display(),
            e.message()
        )
    })?;
    git_common_dir(&repo)
}

fn git_common_dir(repo: &Repository) -> Result<String> {
    let dir = std::fs::canonicalize(repo.commondir())
        .with_context(|| format!("cannot resolve git dir {}", repo.commondir().display()))?;
    dir.into_os_string()
        .into_string()
        .map_err(|_| anyhow!("git dir is not valid UTF-8"))
}

pub(crate) fn discover_repo() -> Result<(String, Repository)> {
    let repo = Repository::discover(".")
        .map_err(|e| anyhow!("not inside a git repository: {}", e.message()))?;
    let git_dir = git_common_dir(&repo)?;
    Ok((git_dir, repo))
}

pub(crate) fn head_sha(repo: &Repository) -> Result<String> {
    let head = repo.head().context("cannot resolve HEAD")?;
    let commit = head.peel_to_commit().context("HEAD is not a commit")?;
    Ok(commit.id().to_string())
}

/// The full sha of the commit to push: the given rev, or the cwd's checked-out
/// commit (HEAD) — a detached HEAD or tag resolved the same way.
pub(crate) fn resolve_tip(repo: &Repository, commit: Option<&str>) -> Result<String> {
    match commit {
        Some(rev) => repo
            .revparse_single(rev)
            .and_then(|obj| obj.peel_to_commit())
            .map(|c| c.id().to_string())
            .map_err(|e| anyhow!("cannot resolve '{rev}': {}", e.message())),
        None => head_sha(repo),
    }
}
