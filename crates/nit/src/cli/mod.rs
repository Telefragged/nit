//! `nit push` / `status` / `log` / `comment` / `reopen` — thin CLI
//! clients of the HTTP API, run by coding agents from inside a git repo
//! (docs/agent-workflow.md). They print API JSON to stdout and decide purely on
//! the documented shapes; all review logic lives server-side.
//!
//! A chain is addressed by its **tip change id**. `nit status`/`nit log`
//! resolve the cwd's tip change from local HEAD; `nit comment` targets a change
//! directly. The live followers `nit wait` / `nit log --follow` watch the
//! cwd's chain over the websocket change stream (docs/api.md "Events").
//!
//! Modules: shared infrastructure (`client` transport, `git` discovery,
//! `resolve` id-lookup, `format` digests) plus one module per subcommand group.

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
mod version;
mod wait;

pub use comment::{CommentArgs, comment};
pub use lifecycle::{AbandonArgs, ReopenArgs, abandon, reopen};
pub use log::{LogArgs, log};
pub use push::{PushArgs, push};
pub use repo::{RepoArgs, repo};
pub use status::{StatusArgs, status};
pub use version::version;
pub use wait::{WaitArgs, wait};
