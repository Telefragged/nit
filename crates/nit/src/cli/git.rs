//! Local git helpers.
//!
//! Discover the cwd's repo, resolve its `git-common-dir` and worktree, read
//! HEAD, and resolve an explicit revision to a full commit sha.

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

/// The repo's checkout directory, canonicalized.
///
/// `None` for a bare repo, or a path that won't canonicalize or isn't UTF-8.
/// Distinct from the git-common-dir, which a linked worktree shares with the
/// main checkout.
pub(crate) fn canonical_workdir(repo: &Repository) -> Option<String> {
    let dir = std::fs::canonicalize(repo.workdir()?).ok()?;
    dir.into_os_string().into_string().ok()
}

/// The branch HEAD points at.
///
/// `None` on a detached HEAD, an unborn branch, or when the head state can't
/// be read — every case where no branch name describes the checkout.
pub(crate) fn head_branch(repo: &Repository) -> Option<String> {
    // A detached HEAD resolves to a reference named `HEAD`, which is not a
    // branch, and an unborn one resolves to nothing at all.
    let head = repo.head().ok()?;
    head.is_branch()
        .then(|| head.shorthand())?
        .map(str::to_string)
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

/// The full sha of the commit to push.
///
/// The given revision, or the cwd's checked-out commit (HEAD) — a detached HEAD or
/// tag resolved the same way.
pub(crate) fn resolve_tip(repo: &Repository, commit: Option<&str>) -> Result<String> {
    match commit {
        Some(revision) => repo
            .revparse_single(revision)
            .and_then(|obj| obj.peel_to_commit())
            .map(|c| c.id().to_string())
            .map_err(|e| anyhow!("cannot resolve '{revision}': {}", e.message())),
        None => head_sha(repo),
    }
}
