//! The tags a checkout has.
//!
//! `nit push` puts them on the changes it registers. `nit status` and
//! `nit log` read by one of them when the user gives no `--tag`.

use git2::Repository;
use nit_types::domain::{Tag, Tags};

use super::git::{canonical_workdir, head_branch};

/// The environment variable Claude Code exports into every command it runs.
///
/// The tag key stays generic, so another harness passes its own id through
/// `--tag session-id=…`.
const SESSION_ID_VAR: &str = "CLAUDE_CODE_SESSION_ID";

/// The tags the checkout has: `branch`, `session-id`, `worktree`.
///
/// `with_branch = false` leaves `branch` out. A push of an explicit commit
/// uses that, because the commit may be on any branch.
pub(crate) fn observed_tags(repo: &Repository, with_branch: bool) -> Tags {
    [
        with_branch.then(|| branch_tag(repo)).flatten(),
        session_tag(),
        worktree_tag(repo),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The tag a command reads by when the user gives no `--tag`.
///
/// The harness session, else the worktree, else the branch HEAD points
/// at. The session and the worktree come first because an agent works
/// in one of each, whatever branch it has checked out. `None` when the
/// checkout has none of the three: a detached HEAD in a bare repo outside
/// a harness.
pub(crate) fn selection_tag(repo: &Repository) -> Option<Tag> {
    session_tag()
        .or_else(|| worktree_tag(repo))
        .or_else(|| branch_tag(repo))
}

/// `branch=<the branch HEAD points at>`, or `None` on a detached HEAD.
fn branch_tag(repo: &Repository) -> Option<Tag> {
    observed("branch", head_branch(repo))
}

/// `session-id=<the harness session>`, or `None` outside a harness.
fn session_tag() -> Option<Tag> {
    observed(
        "session-id",
        std::env::var(SESSION_ID_VAR)
            .ok()
            .filter(|id| !id.is_empty()),
    )
}

/// `worktree=<the canonical workdir>`, or `None` for a bare repo.
fn worktree_tag(repo: &Repository) -> Option<Tag> {
    observed("worktree", canonical_workdir(repo))
}

/// Builds the tag, or returns `None` when `Tag::new` rejects the value.
///
/// A rejected value must not fail the command, because the user did not
/// type it. A path with a control character is the realistic case.
fn observed(key: &str, value: Option<String>) -> Option<Tag> {
    Tag::new(key, value?).ok()
}
