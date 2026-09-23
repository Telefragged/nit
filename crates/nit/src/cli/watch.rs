//! `nit watch` — send the reviewer's entries to this session's agent.
//!
//! It follows the log of the session's changes and posts each run of
//! the reviewer's entries to the session's inbox socket, which starts a
//! turn in an idle session.
//! Claude Code documents that socket and exports its path and token to
//! every command it runs.
//!
//! The agent runs it as a background command and leaves it running, so it
//! stays a child of the session. That is what lets the harness read the
//! messages as the session's own, rather than as a peer's.
//!
//! The follower reads the server on a thread of its own. It hands each
//! batch of new entries to the posting loop over a channel, so that loop
//! can wait for the next entry with a deadline while the follower blocks
//! on the websocket.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use nit_types::domain::{LogEntry, LogPayload};
use nit_types::events::{StreamMessage, Subscription};

use super::client::{Client, Retry, ServerOpt, next_text, retry_delay, server_url};
use super::format::render_entries;
use super::log::dropped_by_incoming;
use super::resolve::{SelectArgs, Selection};
use super::snippet::Sources;

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

/// How many batches the follower may read ahead of the inbox.
const QUEUE: usize = 32;

/// How long the watch waits for another entry before it posts.
///
/// One reviewer action can write several entries, and a message per
/// entry would start a turn per entry.
const QUIET: Duration = Duration::from_millis(200);

/// The longest the watch delays a post while entries keep arriving.
const CEILING: Duration = Duration::from_secs(1);

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
/// When no inbox is given and the harness exports none, when the lock or
/// the cursor can't be read, or when the server returns a malformed
/// response.
pub fn watch(args: WatchArgs) -> Result<()> {
    let inbox = Inbox::open(args.inbox)?;
    // Before the git discovery and the repo lookup, which a second watch
    // would only throw away.
    let Some(_held) = lock(&inbox.path, &args.select)? else {
        println!("another nit watch already holds this session");
        return Ok(());
    };
    let cursor = Cursor::open(&inbox.path, &args.select)?;
    let client = Client::new(server_url(args.server.server));
    // The watch rides out a server restart for its whole life, so it
    // waits for a server that is not up yet either.
    let selection = args.select.resolve(&client, Retry::UntilUp)?;

    let (entries, batches) = sync_channel(QUEUE);
    let start = cursor.read()?;
    let follower_client = client.clone();
    let follower = std::thread::spawn(move || {
        Follower {
            client: follower_client,
            selection,
            entries,
            cursor: start,
        }
        .run()
    });

    deliver(&inbox, &batches, &cursor, &client)?;
    match follower.join() {
        Ok(Err(e)) => Err(e),
        Ok(Ok(never)) => match never {},
        Err(_) => Err(anyhow!("the watch's follower panicked")),
    }
}

/// Posts each run of entries to the inbox, one message per run, with the
/// lines that its line comments anchor to.
///
/// A run ends [`QUIET`] after its last entry, or [`CEILING`] after its
/// first, whichever comes first. Returns when the follower drops the
/// channel, having posted what it had in hand.
///
/// # Errors
///
/// When the inbox refuses a message, the server rejects a request for
/// the lines, or the cursor can't be written.
fn deliver(
    inbox: &Inbox,
    batches: &Receiver<Vec<LogEntry>>,
    cursor: &Cursor,
    client: &Client,
) -> Result<()> {
    while let Ok(first) = batches.recv() {
        let mut run = first;
        let ceiling = Instant::now() + CEILING;
        // The wait is the quiet window, cut short by what is left of the
        // ceiling, so an entry puts the window back to its full length
        // and the run still ends at the ceiling.
        while let Ok(batch) =
            batches.recv_timeout(QUIET.min(ceiling.saturating_duration_since(Instant::now())))
        {
            run.extend(batch);
        }
        inbox.post(&run, &Sources::fetch(client, &run, Retry::UntilUp)?)?;
        cursor.write(max_seq(&run))?;
    }
    Ok(())
}

/// The selection being followed, and where its new entries go.
struct Follower {
    client: Client,
    selection: Selection,
    entries: SyncSender<Vec<LogEntry>>,
    /// The highest `sequence` seen. Entries above it are new.
    cursor: u64,
}

impl Follower {
    /// A follower rides out a server restart.
    const RETRY: Retry = Retry::UntilUp;

    /// Sends the selection's entries past the cursor, then each new one
    /// as it arrives. Returns only by failing.
    ///
    /// It reads the log past the cursor and sends the new entries. Then
    /// it opens a websocket and sends each entry the server writes. A
    /// `tags` entry can be the first the socket sends for a change,
    /// because the change got the tag with that entry, and its earlier
    /// entries exist only in the log. So a `tags` frame makes it read the
    /// log past the cursor again. A closed socket (a server restart) makes
    /// it start over.
    ///
    /// The author's own entries move the cursor but count as nothing
    /// new, so only a review or a lifecycle change reaches the agent.
    ///
    /// # Errors
    ///
    /// When the server returns a malformed response, a fatal client
    /// error, or the posting loop is gone.
    fn run(mut self) -> Result<std::convert::Infallible> {
        // A server that accepts the socket and then drops it, which an
        // overflowed broadcast does, would otherwise spin the reconnect.
        let mut silent_reconnects = 0;
        loop {
            let entries = self
                .selection
                .log(&self.client, Some(self.cursor), Self::RETRY)?;
            self.send_new(entries)?;
            // The socket sends only entries past the cursor, and the caller
            // has taken everything up to it.
            let subscription = Subscription {
                query: self.selection.change_query(),
                after: Some(self.cursor),
            };
            let mut socket = self.client.ws_connect(&subscription)?;
            let mut sent_anything = false;
            while let Some(text) = next_text(&mut socket) {
                sent_anything = true;
                let Ok(StreamMessage::Entry(entry)) = serde_json::from_str::<StreamMessage>(&text)
                else {
                    continue;
                };
                let entries = if matches!(entry.payload, LogPayload::Tags(_)) {
                    self.selection
                        .log(&self.client, Some(self.cursor), Self::RETRY)?
                } else {
                    vec![entry]
                };
                self.send_new(entries)?;
            }
            if sent_anything {
                silent_reconnects = 0;
            } else {
                std::thread::sleep(retry_delay(silent_reconnects));
                silent_reconnects += 1;
            }
        }
    }

    /// Sends the entries past the cursor and moves the cursor past them.
    fn send_new(&mut self, entries: Vec<LogEntry>) -> Result<()> {
        let since = self.cursor;
        self.cursor = max_seq(&entries).max(since);
        let fresh: Vec<LogEntry> = entries
            .into_iter()
            .filter(|e| e.sequence > since)
            .filter(|e| !dropped_by_incoming(e))
            .collect();
        if fresh.is_empty() {
            return Ok(());
        }
        self.entries
            .send(fresh)
            .map_err(|_| anyhow!("the watch stopped posting"))
    }
}

fn max_seq(entries: &[LogEntry]) -> u64 {
    entries.iter().map(|e| e.sequence).max().unwrap_or(0)
}

/// The highest `sequence` the watch has posted, kept across restarts.
///
/// A restarted watch resumes from it, rather than posting every entry
/// the selection ever collected. It carries the lock's key, so the watch
/// that holds the lock is the only one that writes it.
struct Cursor {
    path: PathBuf,
}

impl Cursor {
    /// The file this watch's cursor lives in.
    ///
    /// # Errors
    ///
    /// When the state directory can't be created.
    fn open(socket: &std::path::Path, select: &SelectArgs) -> Result<Cursor> {
        Ok(Cursor {
            path: state_path(socket, select, "cursor")?,
        })
    }

    /// The stored cursor, or 0 when no watch has posted for this
    /// selection yet.
    ///
    /// # Errors
    ///
    /// When the file exists and can't be read or parsed, because
    /// starting over would repost entries the agent has seen.
    fn read(&self) -> Result<u64> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e).with_context(|| format!("read {}", self.path.display())),
        };
        text.trim()
            .parse()
            .with_context(|| format!("read the cursor in {}", self.path.display()))
    }

    /// Stores the cursor, once its message is on the socket.
    ///
    /// # Errors
    ///
    /// When the file can't be written.
    fn write(&self, sequence: u64) -> Result<()> {
        // Written beside the cursor and renamed, because the read refuses
        // a half-written one and no later watch would start.
        let staged = self.path.with_extension("new");
        std::fs::write(&staged, sequence.to_string())
            .with_context(|| format!("write {}", staged.display()))?;
        std::fs::rename(&staged, &self.path).with_context(|| format!("rename {}", staged.display()))
    }
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

    /// Posts the entries as one message, which starts a turn in an idle
    /// session.
    ///
    /// The connection opens only now, because the harness drops one that
    /// sends no complete line within 30 seconds.
    fn post(&self, entries: &[LogEntry], sources: &Sources) -> Result<()> {
        let text = format!("{LEAD}\n\n{}", render_entries(entries, sources));
        let mut socket = UnixStream::connect(&self.path)
            .with_context(|| format!("connect to {}", self.path.display()))?;
        if let Some(auth) = &self.auth {
            writeln!(socket, "{auth}")?;
        }
        let post = serde_json::to_vec(&Post {
            kind: "user",
            message: Body { content: &text },
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

/// One of the watch's state files, named for the inbox and the selection.
///
/// Two watches of different changes can share a session, and each gets
/// files of its own. The files live in the temporary directory, because
/// a session lasts no longer than the machine's uptime.
///
/// # Errors
///
/// When the directory can't be created.
fn state_path(socket: &std::path::Path, select: &SelectArgs, extension: &str) -> Result<PathBuf> {
    let mut key = socket.as_os_str().as_encoded_bytes().to_vec();
    for tag in &select.tag {
        // The zero byte keeps one pair's end apart from the next one's
        // start, so two different selections cannot spell one key.
        key.push(0);
        key.extend_from_slice(tag.key().as_bytes());
        key.push(0);
        key.extend_from_slice(tag.value().as_bytes());
    }
    let dir = std::env::temp_dir().join("nit-watch");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir.join(format!("{:016x}.{extension}", fnv1a(&key))))
}

/// The 64-bit FNV-1a hash of `bytes`.
///
/// The watch names its files after this. The hash is written out here
/// rather than taken from `DefaultHasher`, whose algorithm may change
/// between Rust releases. A name that changes loses the cursor, and the
/// next watch reposts the log the agent has already read.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

/// Claims this watch, or returns `None` when one already runs.
///
/// The claim is an exclusive lock on the watch's own file. The operating
/// system drops the lock when the process ends, so a watch that dies with
/// its session leaves nothing stale behind.
///
/// # Errors
///
/// When the lock file can't be created or read.
fn lock(socket: &std::path::Path, select: &SelectArgs) -> Result<Option<File>> {
    let path = state_path(socket, select, "lock")?;
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

#[cfg(test)]
mod tests {
    use super::fnv1a;

    /// The published FNV-1a vectors, which pin the file names a watch
    /// looks for.
    #[test]
    fn fnv1a_matches_the_published_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
