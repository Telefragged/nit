//! `nit watch` — send the reviewer's entries to this session's agent.
//!
//! It follows the log of the session's changes and posts each run of
//! the reviewer's entries to the session's inbox socket, which starts a
//! turn in an idle session.
//! Claude Code documents that socket and exports its path and token to
//! every command it runs.
//!
//! The plugin's `SessionStart` hook starts it as a child of the session.
//! That is what lets the harness read the messages as the session's own,
//! rather than as a peer's.
//!
//! The follower reads the server on a thread of its own. It hands each
//! batch of new entries to the posting loop over a channel, so that loop
//! can wait for the next entry with a deadline while the follower blocks
//! on the websocket.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use nit_types::domain::{ChangeNumber, LogEntry};
use nit_types::events::{Start, StreamMessage, Subscription};

use crate::db::nit_data_dir;

use super::client::{Client, Retry, ServerOpt, Unreachable, next_text, retry_delay, server_url};
use super::format::render_entries;
use super::log::dropped_by_incoming;
use super::parent::exit_with_parent;
use super::resolve::{NotRegistered, SelectArgs, Selection};
use super::snippet::Sources;
use super::tags::session_id;

/// The position of the next entry of each change.
type Positions = BTreeMap<ChangeNumber, u64>;

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

/// How long a cursor lasts without a write before a watch deletes it.
///
/// Claude Code keeps a session for 30 days by default, so a session that
/// can still resume finds its cursor.
const CURSOR_LIFETIME: Duration = Duration::from_hours(30 * 24);

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
/// Returns without watching when another watch holds this session, when
/// the checkout selects nothing to watch, and, without output, when the
/// server is down or has not registered the repo. Exits the process when
/// its parent process exits.
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
    let client = Client::new(server_url(args.server.server));
    // The plugin starts a watch in every session, in every repo.
    let selection = match args.select.resolve(&client, Retry::No) {
        Ok(selection) => selection,
        Err(e) if e.is::<Unreachable>() || e.is::<NotRegistered>() => return Ok(()),
        Err(e) => return Err(e),
    };
    // The parent is the session, or a shell that lives as long as it does.
    exit_with_parent()?;
    let cursor = Cursor::open(&inbox.path, &args.select)?;

    let (entries, batches) = sync_channel(QUEUE);
    let posted = cursor.read()?;
    let follower_client = client.clone();
    let positions = posted.clone();
    let follower = std::thread::spawn(move || {
        Follower {
            client: follower_client,
            selection,
            entries,
            positions,
        }
        .run()
    });

    deliver(&inbox, &batches, &cursor, posted, &client)?;
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
    mut posted: Positions,
    client: &Client,
) -> Result<()> {
    while let Some(run) = next_run(batches) {
        inbox.post(&run, &Sources::fetch(client, &run, Retry::UntilUp)?)?;
        for entry in &run {
            posted.insert(entry.change_number, entry.position + 1);
        }
        cursor.write(&posted)?;
    }
    Ok(())
}

/// The next run of entries, or `None` once the follower is gone.
///
/// A run opens with the next batch. It ends [`QUIET`] after its last
/// batch, or [`CEILING`] after its first, whichever comes first.
fn next_run(batches: &Receiver<Vec<LogEntry>>) -> Option<Vec<LogEntry>> {
    let mut run = batches.recv().ok()?;
    let ceiling = Instant::now() + CEILING;
    // The wait is the quiet window, cut short by what is left of the
    // ceiling, so a batch puts the window back to its full length and the
    // run still ends at the ceiling.
    while let Ok(batch) =
        batches.recv_timeout(QUIET.min(ceiling.saturating_duration_since(Instant::now())))
    {
        run.extend(batch);
    }
    Some(run)
}

/// The selection being followed, and where its new entries go.
struct Follower {
    client: Client,
    selection: Selection,
    entries: SyncSender<Vec<LogEntry>>,
    /// The position of the next entry to receive of each followed change.
    positions: Positions,
}

impl Follower {
    /// Sends each new entry of the selection to the posting loop. Returns
    /// only by failing.
    ///
    /// It subscribes with the positions that it holds, so the server sends
    /// the entries from them on, and every entry of a change that it lacks. A
    /// closed socket, for example on a server restart, makes it subscribe
    /// again.
    ///
    /// The author's own entries move the positions, and the follower
    /// discards them. Thus the agent receives reviews and lifecycle
    /// changes.
    ///
    /// # Errors
    ///
    /// When the server returns a malformed response, a fatal client
    /// error, or the posting loop is gone.
    fn run(mut self) -> Result<std::convert::Infallible> {
        // A server can accept the socket and then close it at once, for
        // example when its log read fails. The delay below spaces the
        // reconnects for that case.
        let mut silent_reconnects = 0;
        loop {
            let subscription = Subscription {
                query: self.selection.change_query(),
                positions: self.positions.clone(),
                start: Start::Log,
            };
            let mut socket = self.client.ws_connect(&subscription)?;
            let mut sent_anything = false;
            while let Some(text) = next_text(&mut socket) {
                sent_anything = true;
                let Ok(message) = serde_json::from_str::<StreamMessage>(&text) else {
                    continue;
                };
                self.receive(message)?;
            }
            if sent_anything {
                silent_reconnects = 0;
            } else {
                std::thread::sleep(retry_delay(silent_reconnects));
                silent_reconnects += 1;
            }
        }
    }

    /// Moves the position of the change of a new entry. Sends a new entry
    /// of the reviewer to the posting loop.
    ///
    /// The follower keeps the position of a change that leaves the
    /// selection. The server sends every entry of a change that matches
    /// again, and the follower discards the entries that it received
    /// before.
    fn receive(&mut self, message: StreamMessage) -> Result<()> {
        let StreamMessage::Entry(entry) = message else {
            return Ok(());
        };
        let next = self.positions.entry(entry.change_number).or_default();
        if entry.position < *next {
            return Ok(());
        }
        *next = entry.position + 1;
        if dropped_by_incoming(&entry) {
            return Ok(());
        }
        self.entries
            .send(vec![entry])
            .map_err(|_| anyhow!("the watch stopped posting"))
    }
}

/// The position of the next entry to post of each change, kept across
/// restarts.
///
/// A restarted watch subscribes from these positions, so it posts no
/// entry twice. It carries the lock's key, so the watch
/// that holds the lock is the only one that writes it.
struct Cursor {
    path: PathBuf,
}

impl Cursor {
    /// The file this watch's cursor lives in.
    ///
    /// Deletes every cursor older than [`CURSOR_LIFETIME`] first, because
    /// each session leaves one behind.
    ///
    /// The cursors live in nit's data directory, because a session
    /// outlives a reboot, and a sandbox gives each session its own
    /// temporary directory.
    ///
    /// # Errors
    ///
    /// When the data directory is unknown, or can't be created or cleaned.
    fn open(inbox: &Path, select: &SelectArgs) -> Result<Cursor> {
        let dir = nit_data_dir()?.join("watch");
        let path = state_path(&dir, inbox, select, "positions")?;
        prune(&dir, SystemTime::now() - CURSOR_LIFETIME)?;
        Ok(Cursor { path })
    }

    /// The stored positions, or none when no watch has posted for this
    /// selection yet.
    ///
    /// # Errors
    ///
    /// When the file exists and can't be read or parsed, because
    /// starting over would repost entries the agent has seen.
    fn read(&self) -> Result<Positions> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Positions::new()),
            Err(e) => return Err(e).with_context(|| format!("read {}", self.path.display())),
        };
        serde_json::from_str(&text)
            .with_context(|| format!("read the cursor in {}", self.path.display()))
    }

    /// Stores the cursor, once its message is on the socket.
    ///
    /// # Errors
    ///
    /// When the file can't be written.
    fn write(&self, positions: &Positions) -> Result<()> {
        // Written beside the cursor and renamed, because the read refuses
        // a half-written one and no later watch would start.
        let staged = self.path.with_extension("new");
        std::fs::write(&staged, serde_json::to_string(positions)?)
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

/// One of the watch's state files in `dir`, named for the session and
/// the selection.
///
/// The session is the harness session id, else the inbox. The harness
/// names the inbox after its process, so a restarted session gets a new
/// inbox, but it keeps its id.
///
/// Two watches of different changes can share a session, and each gets
/// files of its own.
///
/// # Errors
///
/// When `dir` can't be created.
fn state_path(dir: &Path, inbox: &Path, select: &SelectArgs, extension: &str) -> Result<PathBuf> {
    let mut key = match session_id() {
        Some(id) => id.into_bytes(),
        None => inbox.as_os_str().as_encoded_bytes().to_vec(),
    };
    for tag in &select.tag {
        // The zero byte keeps one pair's end apart from the next one's
        // start, so two different selections cannot spell one key.
        key.push(0);
        key.extend_from_slice(tag.key().as_bytes());
        key.push(0);
        key.extend_from_slice(tag.value().as_bytes());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
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
/// its session leaves nothing stale behind. The file lives in the
/// temporary directory, because it matters only while a watch runs.
///
/// # Errors
///
/// When the lock file can't be created or read.
fn lock(inbox: &Path, select: &SelectArgs) -> Result<Option<File>> {
    let dir = std::env::temp_dir().join("nit-watch");
    let path = state_path(&dir, inbox, select, "lock")?;
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

/// Deletes the files in `dir` last written before `cutoff`.
///
/// # Errors
///
/// When `dir` can't be read, or a file can't be deleted.
fn prune(dir: &Path, cutoff: SystemTime) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        if entry.metadata()?.modified()? >= cutoff {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            // Another watch, starting at the same moment, deleted it first.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            result => result.with_context(|| format!("delete {}", entry.path().display()))?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::sync_channel;

    use nit_types::domain::{ChangeNumber, LogPayload, Tags, TagsPayload};

    use std::time::{Duration, SystemTime};

    use super::{Cursor, LogEntry, Positions, fnv1a, next_run, prune};

    fn entry(position: u64) -> LogEntry {
        LogEntry {
            change_number: ChangeNumber::new(1),
            position,
            created_at: String::new(),
            payload: LogPayload::Tags(TagsPayload { tags: Tags::new() }),
        }
    }

    fn cursor(dir: &tempfile::TempDir) -> Cursor {
        Cursor {
            path: dir.path().join("watch.cursor"),
        }
    }

    /// The published FNV-1a vectors, which pin the file names a watch
    /// looks for.
    #[test]
    fn fnv1a_matches_the_published_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    /// The batches waiting in the channel make one run, which is what
    /// sends one message per reviewer pass. The closed channel ends the
    /// run at once, so this waits out no window.
    #[test]
    fn one_run_takes_every_waiting_batch() {
        let (entries, batches) = sync_channel(4);
        entries.send(vec![entry(1)]).expect("send the first batch");
        entries
            .send(vec![entry(2), entry(3)])
            .expect("send the second batch");
        drop(entries);

        let run = next_run(&batches).expect("a run");
        let positions: Vec<u64> = run.iter().map(|e| e.position).collect();
        assert_eq!(positions, [1, 2, 3]);
        assert!(next_run(&batches).is_none(), "the follower is gone");
    }

    #[test]
    fn the_cursor_reads_back_what_it_wrote() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cursor = cursor(&dir);
        assert_eq!(cursor.read().expect("a missing cursor"), Positions::new());
        let positions = Positions::from([(ChangeNumber::new(7), 42)]);
        cursor.write(&positions).expect("write the cursor");
        assert_eq!(cursor.read().expect("read the cursor"), positions);
    }

    /// A half-written cursor stops the watch, rather than reading as 0
    /// and reposting the log the agent has already read.
    #[test]
    fn a_truncated_cursor_fails_the_read() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cursor = cursor(&dir);
        std::fs::write(&cursor.path, "").expect("truncate the cursor");
        assert!(cursor.read().is_err(), "an empty cursor reads as an error");
    }

    #[test]
    fn prune_deletes_only_the_files_written_before_the_cutoff() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cutoff = SystemTime::now() - Duration::from_mins(1);
        let old = dir.path().join("old.cursor");
        let fresh = dir.path().join("fresh.cursor");
        std::fs::File::create(&old)
            .and_then(|file| file.set_modified(cutoff - Duration::from_secs(1)))
            .expect("write the old cursor");
        std::fs::write(&fresh, "1").expect("write the fresh cursor");

        prune(dir.path(), cutoff).expect("prune");
        assert!(!old.exists(), "the old cursor is gone");
        assert!(fresh.exists(), "the fresh cursor stays");
    }
}
