//! Thin CLI clients of the HTTP API, run by coding agents.
//!
//! `nit push` / `status` / `log` / `comment` / `attach` / `reopen`, run from inside a
//! git repo. They print concise text for the author to act on; all review
//! logic lives server-side.
//!
//! `nit status` reads the changes that have a tag. The tag comes from
//! `--tag`, or else from the checkout: its harness session, else its
//! worktree, else its branch. `nit log` and `nit comment` name a change
//! directly.
//! `nit watch` reads the same changes and then follows new entries on the
//! websocket, posting each one to the session that runs it.
//!
//! Modules: shared infrastructure (`client` transport, `git` discovery,
//! `tags` the checkout's tags, `resolve` the selection, `format` digests)
//! plus one module per subcommand group.

mod attach;
mod client;
mod comment;
mod format;
mod gc;
mod git;
mod lifecycle;
mod log;
mod parent;
mod plugin;
mod push;
mod repo;
mod resolve;
mod snippet;
mod status;
mod tags;
mod update;
mod version;
mod watch;

pub use attach::{AttachArgs, attach};
pub use comment::{CommentArgs, comment};
pub use gc::{GcArgs, gc};
pub use lifecycle::{AbandonArgs, ReopenArgs, abandon, reopen};
pub use log::{LogArgs, log};
pub use plugin::install_plugin;
pub use push::{PushArgs, push};
pub use repo::{RepoArgs, repo};
pub use status::{StatusArgs, status};
pub use update::update;
pub use version::version;
pub use watch::{WatchArgs, watch};
