//! `nit watch` — send the reviewer's entries to this session's agent.
//!
//! It follows the log of the session's changes and posts each batch of
//! the reviewer's entries to the session's inbox socket, which starts a
//! turn in an idle session.
//! Claude Code documents that socket and exports its path and token to
//! every command it runs.
//!
//! The agent runs it as a background command and leaves it running, so it
//! stays a child of the session. That is what lets the harness read the
//! messages as the session's own, rather than as a peer's.

use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use super::client::{Client, Retry, ServerOpt, server_url};
use super::log::follow;
use super::resolve::SelectArgs;

/// What Claude Code calls the inbox socket it exports to every command.
const SOCKET_VAR: &str = "CLAUDE_CODE_MESSAGING_SOCKET";
/// The token that proves a message comes from the session's own child.
const TOKEN_VAR: &str = "CLAUDE_CODE_MESSAGING_TOKEN";

/// What the agent reads above the entries.
///
/// It names the sender, because the harness labels anything posted to
/// this socket as a message from another Claude session.
const LEAD: &str = "A reviewer acted on your changes. nit sent this, \
not another Claude session. Act on it now, the way the nit:lifecycle \
skill says.";

#[derive(clap::Args)]
pub struct WatchArgs {
    /// The session inbox to post to (default: `$CLAUDE_CODE_MESSAGING_SOCKET`).
    #[arg(long)]
    pub inbox: Option<PathBuf>,
    #[command(flatten)]
    pub select: SelectArgs,
    #[command(flatten)]
    pub server: ServerOpt,
}

/// Follows the session's changes and posts each new entry to its agent.
///
/// Returns without watching when another watch holds this session, or
/// when the checkout selects nothing to watch.
///
/// # Errors
///
/// When no inbox is given and the harness exports none, when the lock
/// can't be taken, or when the server returns a malformed response.
pub fn watch(args: WatchArgs) -> Result<()> {
    let inbox = Inbox::open(args.inbox)?;
    // Before the git discovery and the repo lookup, which a second watch
    // would only throw away.
    let Some(_held) = lock(&inbox.path, &args.select)? else {
        println!("another nit watch already holds this session");
        return Ok(());
    };
    let client = Client::new(server_url(args.server.server));
    // The watch rides out a server restart for its whole life, so it
    // waits for a server that is not up yet either.
    let selection = args.select.resolve(&client, Retry::UntilUp)?;
    follow(&client, &selection, true, &mut |text| {
        inbox.post(&format!("{LEAD}\n\n{text}"))
    })
}

/// The session's inbox: where to post, and what proves who posts.
struct Inbox {
    path: PathBuf,
    /// The opening line of every connection, built once.
    auth: Option<String>,
}

/// One message, as the harness reads it off the socket.
#[derive(Serialize)]
struct Post<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    message: Body<'a>,
}

#[derive(Serialize)]
struct Body<'a> {
    content: &'a str,
}

#[derive(Serialize)]
struct Auth<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    token: &'a str,
}

impl Inbox {
    /// The inbox the caller named, else the one the harness exported.
    ///
    /// # Errors
    ///
    /// When neither gives a path, so there is nowhere to post.
    fn open(flag: Option<PathBuf>) -> Result<Inbox> {
        let path = if let Some(path) = flag {
            path
        } else {
            let exported = harness_var(SOCKET_VAR).ok_or_else(|| {
                anyhow!("no inbox: pass --inbox, or run where ${SOCKET_VAR} is set")
            })?;
            // `/status` spells the same address with this prefix.
            PathBuf::from(exported.strip_prefix("uds:").unwrap_or(&exported))
        };
        let auth = harness_var(TOKEN_VAR)
            .map(|token| {
                serde_json::to_string(&Auth {
                    kind: "auth",
                    token: &token,
                })
            })
            .transpose()?;
        Ok(Inbox { path, auth })
    }

    /// Posts one message, which starts a turn in an idle session.
    ///
    /// The connection opens only now, because the harness drops one that
    /// sends no complete line within 30 seconds.
    fn post(&self, text: &str) -> Result<()> {
        let mut socket = UnixStream::connect(&self.path)
            .with_context(|| format!("connect to {}", self.path.display()))?;
        if let Some(auth) = &self.auth {
            writeln!(socket, "{auth}")?;
        }
        let post = serde_json::to_vec(&Post {
            kind: "user",
            message: Body { content: text },
        })?;
        socket.write_all(&post)?;
        socket.write_all(b"\n")?;
        Ok(())
    }
}

/// The harness variable, or `None` when it is unset or empty.
fn harness_var(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

/// Claims this watch, or returns `None` when one already runs.
///
/// The claim is an exclusive lock on a file named for the inbox and the
/// selection, so two watches of different changes can share a session.
/// The operating system drops the lock when the process ends, so a watch
/// that dies with its session leaves nothing stale behind. The file lives
/// in the temporary directory because the claim lasts no longer than the
/// machine's uptime.
///
/// # Errors
///
/// When the lock file can't be created or read.
fn lock(socket: &std::path::Path, select: &SelectArgs) -> Result<Option<File>> {
    let mut hasher = DefaultHasher::new();
    socket.hash(&mut hasher);
    for tag in &select.tag {
        tag.key().hash(&mut hasher);
        tag.value().hash(&mut hasher);
    }
    let dir = std::env::temp_dir().join("nit-watch");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(format!("{:016x}.lock", hasher.finish()));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("lock {}", path.display()))
        }
    }
}
