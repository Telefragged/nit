//! Thin CLI clients of the HTTP API, run by coding agents.
//!
//! `nit push` / `status` / `log` / `comment` / `reopen`, run from inside a
//! git repo. They print concise text for the author to act on; all review
//! logic lives server-side.
//!
//! `nit status` and `nit log` read the changes that have a tag. The tag
//! comes from `--tag`, or else from the checkout: its harness session,
//! else its worktree, else its branch. `nit comment` names a change
//! directly.
//! `nit log --follow` and `--wait` read the same changes and then wait for
//! new entries on the websocket.
//!
//! Modules: shared infrastructure (`client` transport, `git` discovery,
//! `tags` the checkout's tags, `resolve` the selection, `format` digests)
//! plus one module per subcommand group.

mod client;
mod comment;
mod format;
mod git;
mod lifecycle;
mod log;
mod push;
mod repo;
mod resolve;
mod status;
mod tags;
mod version;

pub use comment::{CommentArgs, comment};
pub use lifecycle::{AbandonArgs, ReopenArgs, abandon, reopen};
pub use log::{LogArgs, log};
pub use push::{PushArgs, push};
pub use repo::{RepoArgs, repo};
pub use status::{StatusArgs, status};
pub use version::version;
