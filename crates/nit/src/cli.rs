//! `nit push` / `ready` / `status` / `log` / `comment` / `reopen` — thin CLI
//! clients of the HTTP API, run by coding agents from inside a git repo
//! (docs/agent-workflow.md). They print API JSON to stdout and decide purely on
//! the documented shapes; all review logic lives server-side.
//!
//! A chain is addressed by its **tip change id**. `nit status`/`nit log`
//! resolve the cwd's tip change from local HEAD; `nit comment` targets a change
//! directly. The live followers `nit wait` / `nit log --follow` watch the
//! cwd's chain over the websocket change stream (docs/api.md "Events").

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use git2::Repository;
use serde_json::{Value, json};

use crate::api::types::{CommentRange, NewComment};
use crate::enums::Side;
use crate::gitscan::short_sha;

pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8877";

fn server_url(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("NIT_SERVER").ok())
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

// ---------------------------------------------------------------------------
// Args

#[derive(clap::Args)]
pub struct PushArgs {
    /// The commit to push: any rev (sha, tag, branch). Defaults to the
    /// checked-out commit (HEAD) of the cwd — a detached HEAD or tag included.
    pub commit: Option<String>,
    /// The repo's canonical base branch. Detected server-side (`main` or
    /// `master`) when omitted; pass it when neither or both exist.
    #[arg(long)]
    pub base: Option<String>,
    /// Mark the tip partial: review can start, merging cannot; sticky until
    /// `nit ready`
    #[arg(long)]
    pub partial: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct ReadyArgs {
    /// The commit to mark ready (see `nit push`); defaults to the cwd's HEAD.
    pub commit: Option<String>,
    /// The repo's canonical base branch (see `nit push`).
    #[arg(long)]
    pub base: Option<String>,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct LogArgs {
    /// Without `--follow`: entry positions or half-open ranges into the
    /// aggregated chain log (sorted by global seq): `3`, `5..9`, `5..`, `..9`,
    /// `..` (all, the default). With `--follow`: a single global `seq` cursor
    /// to stream from.
    #[arg(default_value = "..")]
    pub ranges: Vec<String>,
    /// Chain to read, by its tip change id; overrides the cwd lookup.
    #[arg(long)]
    pub chain: Option<u64>,
    /// Print a one-line digest per entry instead of full payloads
    #[arg(long)]
    pub oneline: bool,
    /// Follow the log: replay from the cursor, then stream each new entry as it
    /// lands — a parked monitor. Rides out restarts; runs until stopped.
    #[arg(long)]
    pub follow: bool,
    /// With `--follow`, relay only the reviewer's activity: drop the agent's
    /// own entries (`revision`/`comment`/`partial`).
    #[arg(long, requires = "follow")]
    pub reviewer_only: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct WaitArgs {
    /// Global `seq` cursor: the highest log seq already consumed (start at 0,
    /// then pass the `cursor` each result prints; docs/agent-workflow.md).
    pub cursor: u64,
    /// Print a one-line digest per entry instead of full payloads
    #[arg(long)]
    pub oneline: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct StatusArgs {
    /// Chain to read, by its tip change id; overrides the cwd lookup.
    #[arg(long)]
    pub chain: Option<u64>,
    /// Print a compact one-line-per-change digest instead of full JSON
    #[arg(long)]
    pub oneline: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

/// The shared `--change` / `--change-id` selector for change-scoped commands.
#[derive(clap::Args)]
pub struct ChangeTarget {
    /// The change, by its numeric id.
    #[arg(
        long,
        conflicts_with = "change_id",
        required_unless_present = "change_id"
    )]
    pub change: Option<u64>,
    /// The change, by its `Change-Id:` trailer.
    #[arg(long)]
    pub change_id: Option<String>,
}

impl ChangeTarget {
    /// Resolve to a numeric change id, querying the server for a `Change-Id:`.
    fn resolve(&self, client: &Client) -> Result<u64> {
        match (self.change, self.change_id.as_deref()) {
            (Some(id), _) => Ok(id),
            (None, Some(key)) => resolve_change(client, key),
            (None, None) => bail!("pass --change <id> or --change-id <Change-Id>"),
        }
    }
}

#[derive(clap::Args)]
pub struct CommentArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    /// Reply to an existing thread on the change (by id) instead of opening
    /// a new one.
    #[arg(long)]
    pub thread: Option<u64>,
    /// New thread: file to anchor to (a `--line` requires a `--file`).
    #[arg(long, conflicts_with = "thread")]
    pub file: Option<String>,
    /// New thread: line to anchor to (1-based).
    #[arg(long, conflicts_with = "thread")]
    pub line: Option<u64>,
    /// New thread: side — `new` (default) or `old`.
    #[arg(long, conflicts_with = "thread", value_enum)]
    pub side: Option<Side>,
    /// New thread: selected-text range `START-END`, each `line:char`.
    #[arg(long, conflicts_with = "thread")]
    pub range: Option<String>,
    /// New thread: revision to anchor to (defaults to the change's latest).
    #[arg(long, conflicts_with = "thread")]
    pub revision: Option<u64>,
    /// Comment body (optional only for a `--thread` reply that just
    /// resolves/reopens).
    #[arg(short = 'm', long = "message")]
    pub message: Option<String>,
    /// Mark the thread resolved (a new thread is born resolved).
    #[arg(long)]
    pub resolve: bool,
    /// Reopen the thread (mark it unresolved)
    #[arg(long, conflicts_with = "resolve")]
    pub unresolve: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct ReopenArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct AbandonArgs {
    #[command(flatten)]
    pub target: ChangeTarget,
    /// Optional reason recorded on the abandonment.
    #[arg(long, short = 'm')]
    pub message: Option<String>,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

/// `nit repo` — inspect and manage the repository registry.
#[derive(clap::Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub cmd: RepoCmd,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`).
    #[arg(long, global = true)]
    pub server: Option<String>,
}

#[derive(clap::Subcommand)]
pub enum RepoCmd {
    /// List registered repos and their live-tip counts
    List,
    /// Repoint a repo at its new location after moving it on disk
    Move(RepoMoveArgs),
}

#[derive(clap::Args)]
pub struct RepoMoveArgs {
    /// The repo's current path, exactly as `nit repo list` prints its
    /// `git_dir` (or that path with the `/.git` dropped).
    pub from: String,
    /// The repo's new location on disk (a worktree or its `.git` dir).
    pub to: PathBuf,
}

// ---------------------------------------------------------------------------
// Commands

/// Push the cwd's checked-out commit (or an explicit rev) for review;
/// idempotent.
///
/// # Errors
/// When the cwd is not a git repo, the rev can't be resolved, the server is
/// unreachable, or the push is rejected.
pub fn push(args: PushArgs) -> Result<()> {
    do_push(
        args.commit.as_deref(),
        args.base,
        args.server,
        args.partial.then_some(true),
    )
}

/// Mark the chain complete: clear the partial flag set by `nit push --partial`.
///
/// # Errors
/// Same as [`push`].
pub fn ready(args: ReadyArgs) -> Result<()> {
    do_push(args.commit.as_deref(), args.base, args.server, Some(false))
}

/// Shared push/ready core: resolve the cwd's repo + the commit to push, then
/// `POST /api/push`. `base` is sent only when given (else the server detects
/// it); `partial` only when an override is given (absent leaves it unchanged).
fn do_push(
    commit: Option<&str>,
    base: Option<String>,
    server: Option<String>,
    partial: Option<bool>,
) -> Result<()> {
    let (git_dir, repo) = discover_repo()?;
    let tip = resolve_tip(&repo, commit)?;
    let client = Client::new(server_url(server));
    let mut body = json!({"git_dir": git_dir, "tip": tip});
    if let Some(base) = base {
        body["base"] = json!(base);
    }
    if let Some(partial) = partial {
        body["partial"] = json!(partial);
    }
    let result = client.post("/api/push", &body)?;
    print_json(&result)
}

/// The full sha of the commit to push: the given rev, or the cwd's checked-out
/// commit (HEAD) — a detached HEAD or tag resolved the same way.
fn resolve_tip(repo: &Repository, commit: Option<&str>) -> Result<String> {
    match commit {
        Some(rev) => repo
            .revparse_single(rev)
            .and_then(|obj| obj.peel_to_commit())
            .map(|c| c.id().to_string())
            .map_err(|e| anyhow!("cannot resolve '{rev}': {}", e.message())),
        None => head_sha(repo),
    }
}

/// Print the chain's status: the derived state plus one line per member.
///
/// # Errors
/// When the server can't be reached or no chain matches the current branch.
pub fn status(args: StatusArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let change_id = resolve_chain(&client, args.chain, Retry::No)?;
    let chain = client.get(&format!("/api/chains/{change_id}"))?;
    if args.oneline {
        print!("{}", chain_oneline(&chain));
        Ok(())
    } else {
        print_json(&chain)
    }
}

/// Print entries of the aggregated chain log by position/range.
///
/// # Errors
/// When a range is malformed or the server can't be reached.
pub fn log(args: LogArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    if args.follow {
        let [spec] = args.ranges.as_slice() else {
            bail!("--follow takes a single starting seq cursor (e.g. `0` or `..`)");
        };
        let cursor = follow_cursor(spec)?;
        let change_id = resolve_chain(&client, args.chain, Retry::No)?;
        return follow(&client, change_id, cursor, args.oneline, args.reviewer_only);
    }
    let change_id = resolve_chain(&client, args.chain, Retry::No)?;
    let log = client.get(&format!("/api/chains/{change_id}/log"))?;
    let all = log["entries"].as_array().cloned().unwrap_or_default();
    let mut entries: Vec<Value> = Vec::new();
    for spec in &args.ranges {
        let (from, to) = LogRange::parse(spec)?.bounds(all.len());
        entries.extend(all.get(from..to).unwrap_or(&[]).iter().cloned());
    }
    if args.oneline {
        print_oneline_entries(&entries);
    } else {
        print_json(&json!({"entries": entries}))?;
    }
    Ok(())
}

/// Block until the chain's aggregated log holds something worth acting on past
/// the `seq` cursor, then print `{cursor, entries, feedback}`. No timeout — the
/// agent calls this only when it has nothing else to do.
///
/// Each pass **drains `(cursor, head]` from the log** (the log is the source of
/// truth) and returns it if non-empty; otherwise it blocks the websocket as a
/// doorbell — until any new entry lands, then re-drains. Reading from
/// the log rather than the stream is what makes a single `wait` surface *every*
/// entry since the cursor, not just the first. Rides out server restarts.
///
/// # Errors
/// When the server returns a malformed response or a fatal client error.
pub fn wait(args: WaitArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let retry = Retry::UntilUp;
    let mut cursor = args.cursor;
    // HEAD is fixed for this command's lifetime, so the tip change id (the
    // chain's stable identity) resolves once, not every loop pass.
    let tip = resolve_tip_change(&client, retry)?;
    loop {
        let log = client.get_retry(&format!("/api/chains/{tip}/log"), retry)?;
        let entries: Vec<Value> = log["entries"].as_array().cloned().unwrap_or_default();
        let fresh: Vec<Value> = entries
            .iter()
            .filter(|e| e["seq"].as_u64().unwrap_or(0) > cursor)
            .cloned()
            .collect();
        cursor = max_seq(&entries).max(cursor);

        if !fresh.is_empty() {
            let feedback = client.get_retry(&format!("/api/chains/{tip}"), retry)?;
            let resp = json!({"cursor": cursor, "entries": fresh, "feedback": feedback});
            print_wait(&resp, args.oneline)?;
            return Ok(());
        }
        // Nothing new: park on the websocket until the head advances.
        wait_for_entry(&client, &entries, retry)?;
    }
}

/// Park the websocket as a doorbell: subscribe the chain's changes at their
/// current heads (no backlog replay) and block until the first live frame, then
/// return so the caller re-drains the log. Rides out restarts.
fn wait_for_entry(client: &Client, entries: &[Value], retry: Retry) -> Result<()> {
    let subs = heads(entries);
    loop {
        let mut socket = client.ws_connect(&subs, retry)?;
        loop {
            match socket.read() {
                Ok(tungstenite::Message::Text(_)) => return Ok(()), // an entry landed
                Ok(tungstenite::Message::Ping(p)) => {
                    let _ = socket.send(tungstenite::Message::Pong(p));
                }
                Ok(tungstenite::Message::Close(_)) | Err(_) => break, // reconnect
                Ok(_) => {}
            }
        }
    }
}

/// Follow the aggregated chain log as a parked monitor: replay `(cursor, head]`,
/// then relay each new entry as it lands, until stopped. Rides out restarts
/// (reconnect re-reads the gap from the log). `reviewer_only` drops the agent's
/// own entries (`revision`/`comment`/`partial`).
///
/// # Errors
/// When a connect fails fatally or stdout can't be written.
fn follow(
    client: &Client,
    change_id: u64,
    mut cursor: u64,
    oneline: bool,
    reviewer_only: bool,
) -> Result<()> {
    let retry = Retry::UntilUp;
    loop {
        // Re-derive the chain each connect: a new tip enters the watch set, a
        // departed change goes quiet (self-healing, never needs new_parent).
        let log = client.get_retry(&format!("/api/chains/{change_id}/log"), retry)?;
        let entries: Vec<Value> = log["entries"].as_array().cloned().unwrap_or_default();
        for e in &entries {
            if e["seq"].as_u64().unwrap_or(0) > cursor {
                cursor = cursor.max(e["seq"].as_u64().unwrap_or(cursor));
                relay(e, oneline, reviewer_only)?;
            }
        }
        let mut socket = client.ws_connect(&heads(&entries), retry)?;
        loop {
            match socket.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    let Ok(entry) = serde_json::from_str::<Value>(text.as_str()) else {
                        continue;
                    };
                    if entry.get("new_parent").is_some() {
                        break; // re-derive the chain (picks up the new parent)
                    }
                    cursor = cursor.max(entry["seq"].as_u64().unwrap_or(cursor));
                    relay(&entry, oneline, reviewer_only)?;
                }
                Ok(tungstenite::Message::Ping(p)) => {
                    let _ = socket.send(tungstenite::Message::Pong(p));
                }
                Ok(tungstenite::Message::Close(_)) | Err(_) => break, // reconnect
                Ok(_) => {}
            }
        }
    }
}

/// Relay one streamed entry, honoring `--reviewer-only`.
fn relay(entry: &Value, oneline: bool, reviewer_only: bool) -> Result<()> {
    if reviewer_only && is_agent_echo(entry) {
        return Ok(());
    }
    if oneline {
        print_oneline_entries(std::slice::from_ref(entry));
        Ok(())
    } else {
        print_json(entry)
    }
}

/// Each change's head idx (max idx + 1) from the aggregated log — the
/// from-idx to subscribe at so the backlog replay is empty (doorbell mode).
fn heads(entries: &[Value]) -> std::collections::HashMap<u64, u64> {
    let mut heads: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for e in entries {
        if let (Some(cid), Some(idx)) = (e["change_id"].as_u64(), e["idx"].as_u64()) {
            heads
                .entry(cid)
                .and_modify(|h| *h = (*h).max(idx + 1))
                .or_insert(idx + 1);
        }
    }
    heads
}

fn max_seq(entries: &[Value]) -> u64 {
    entries
        .iter()
        .filter_map(|e| e["seq"].as_u64())
        .max()
        .unwrap_or(0)
}

/// Parse the single `--follow` positional into a starting `seq` cursor: a bare
/// `N` follows from `N`, `..` from `0`.
fn follow_cursor(spec: &str) -> Result<u64> {
    let spec = spec.trim();
    if spec == ".." || spec.is_empty() {
        return Ok(0);
    }
    spec.trim_end_matches("..")
        .parse::<u64>()
        .with_context(|| format!("bad seq cursor {spec:?}"))
}

/// A log entry that echoes the agent's own action (`revision`/`comment`/
/// `partial`), suppressed by `--reviewer-only`. Unrecognized kinds fail open.
fn is_agent_echo(entry: &Value) -> bool {
    matches!(
        entry["kind"].as_str(),
        Some("revision" | "comment" | "partial")
    )
}

fn print_wait(resp: &Value, oneline: bool) -> Result<()> {
    if !oneline {
        return print_json(resp);
    }
    let cursor = resp["cursor"].as_u64().unwrap_or(0);
    let state = resp["feedback"]["state"].as_str().unwrap_or("?");
    println!("cursor={cursor} state={state}");
    if let Some(arr) = resp["entries"].as_array() {
        print_oneline_entries(arr);
    }
    Ok(())
}

/// Comment on a change: open a new thread or reply to one.
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn comment(args: CommentArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let change_id = args.target.resolve(&client)?;
    let resolved = if args.resolve {
        Some(true)
    } else if args.unresolve {
        Some(false)
    } else {
        None
    };
    let range = args
        .range
        .map(|spec| parse_comment_range(&spec))
        .transpose()?;
    let req = NewComment {
        thread_id: args.thread,
        revision: args.revision,
        file: args.file,
        line: args.line,
        side: args.side,
        range,
        body: args.message.unwrap_or_default(),
        resolved,
    };
    let thread = client.post(
        &format!("/api/changes/{change_id}/comments"),
        &serde_json::to_value(&req)?,
    )?;
    print_json(&thread)
}

/// Reopen an abandoned change so a new revision may be pushed.
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn reopen(args: ReopenArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let change_id = args.target.resolve(&client)?;
    let detail = client.post(&format!("/api/changes/{change_id}/reopen"), &json!({}))?;
    print_json(&detail)
}

/// Mark a change abandoned — a reviewer/agent judgment that it is dead
/// (reversible by `nit reopen`).
///
/// # Errors
/// When the server can't be reached or the arguments name no change.
pub fn abandon(args: AbandonArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let change_id = args.target.resolve(&client)?;
    let body = match args.message {
        Some(message) => json!({ "message": message }),
        None => json!({}),
    };
    let detail = client.post(&format!("/api/changes/{change_id}/abandon"), &body)?;
    print_json(&detail)
}

/// `nit repo` dispatch.
///
/// # Errors
/// Per subcommand: server unreachable, or (for `move`) an unresolvable path.
pub fn repo(args: RepoArgs) -> Result<()> {
    match args.cmd {
        RepoCmd::List => {
            let client = Client::new(server_url(args.server));
            print_json(&client.get("/api/repos")?)
        }
        RepoCmd::Move(a) => repo_move(&a, args.server),
    }
}

fn repo_move(args: &RepoMoveArgs, server: Option<String>) -> Result<()> {
    let client = Client::new(server_url(server));
    let to = repo_git_dir(&args.to)?;
    let from = args.from.trim_end_matches('/');
    let list = client.get("/api/repos")?;
    let repos = list["repos"]
        .as_array()
        .ok_or_else(|| anyhow!("malformed repo list: {list}"))?;
    let id = repos
        .iter()
        .find(|r| {
            let gd = r["git_dir"].as_str().unwrap_or("");
            gd == from || gd.strip_suffix("/.git").is_some_and(|root| root == from)
        })
        .and_then(|r| r["id"].as_u64())
        .ok_or_else(|| {
            anyhow!("no repo registered at '{from}' — run 'nit repo list' to see the exact paths")
        })?;
    let updated = client.patch(&format!("/api/repos/{id}"), &json!({"git_dir": to}))?;
    print_json(&updated)
}

// ---------------------------------------------------------------------------
// Chain / change resolution (cwd → tip change id)

/// The tip change id for the cwd's repo + branch HEAD: find the chain whose tip
/// commit-sha equals the local HEAD, via `GET /api/chains?status=all`. `retry`
/// covers only the GETs — repo discovery and "not registered" stay fatal.
fn resolve_tip_change(client: &Client, retry: Retry) -> Result<u64> {
    let (git_dir, repo) = discover_repo()?;
    let head = head_sha(&repo)?;
    let repo_id = repo_id_for(client, &git_dir, retry)?;
    let list = client.get_retry(&format!("/api/chains?repo={repo_id}&status=all"), retry)?;
    list["chains"]
        .as_array()
        .ok_or_else(|| anyhow!("malformed chain list: {list}"))?
        .iter()
        .find(|c| {
            c["path"]
                .as_array()
                .and_then(|p| p.last())
                .and_then(|m| m["commit_sha"].as_str())
                == Some(head.as_str())
        })
        .and_then(|c| c["tip_change_id"].as_u64())
        .ok_or_else(|| anyhow!("HEAD is not registered with nit — run 'nit push' first"))
}

/// The chain's tip change id: the explicit `--chain` when given, else the
/// cwd's tip change.
fn resolve_chain(client: &Client, explicit: Option<u64>, retry: Retry) -> Result<u64> {
    match explicit {
        Some(id) => Ok(id),
        None => resolve_tip_change(client, retry),
    }
}

/// The numeric change id for a `Change-Id` trailer on the cwd's chain.
fn resolve_change(client: &Client, change_key: &str) -> Result<u64> {
    let tip = resolve_tip_change(client, Retry::No)?;
    let chain = client.get(&format!("/api/chains/{tip}"))?;
    chain["path"]
        .as_array()
        .and_then(|p| {
            p.iter()
                .find(|m| m["change_key"].as_str() == Some(change_key))
        })
        .and_then(|m| m["change_id"].as_u64())
        .ok_or_else(|| anyhow!("no change with Change-Id {change_key:?} on this chain"))
}

/// The registry id of the repo at `git_dir`.
fn repo_id_for(client: &Client, git_dir: &str, retry: Retry) -> Result<u64> {
    let list = client.get_retry("/api/repos", retry)?;
    list["repos"]
        .as_array()
        .and_then(|rs| rs.iter().find(|r| r["git_dir"].as_str() == Some(git_dir)))
        .and_then(|r| r["id"].as_u64())
        .ok_or_else(|| anyhow!("repo not registered with nit — run 'nit push' first"))
}

// ---------------------------------------------------------------------------
// Log ranges + one-line digests

/// A parsed `nit log` position selector, half-open. `bounds(len)` clamps to the
/// aggregated log's length (positions, not idx/seq).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogRange {
    Open { from: u64 },
    Closed { from: u64, to: u64 },
}

impl LogRange {
    fn parse(spec: &str) -> Result<LogRange> {
        let num = |s: &str| -> Result<u64> {
            s.trim()
                .parse::<u64>()
                .with_context(|| format!("bad index {:?}", s.trim()))
        };
        let Some((a, b)) = spec.split_once("..") else {
            let from = num(spec)?;
            let to = from
                .checked_add(1)
                .ok_or_else(|| anyhow!("index {from} too large"))?;
            return Ok(LogRange::Closed { from, to });
        };
        let from = if a.trim().is_empty() { 0 } else { num(a)? };
        if b.trim().is_empty() {
            return Ok(LogRange::Open { from });
        }
        let to = num(b)?;
        if to <= from {
            bail!("empty or reversed range {spec:?}: the end must be greater than the start");
        }
        Ok(LogRange::Closed { from, to })
    }

    fn bounds(self, len: usize) -> (usize, usize) {
        let clamp = |n: u64| usize::try_from(n).unwrap_or(usize::MAX).min(len);
        match self {
            LogRange::Open { from } => (clamp(from), len),
            LogRange::Closed { from, to } => {
                let from = clamp(from);
                (from, clamp(to).max(from))
            }
        }
    }
}

fn print_oneline_entries(entries: &[Value]) {
    for e in entries {
        let idx = e["idx"]
            .as_u64()
            .map_or_else(|| "?".to_string(), |i| i.to_string());
        let kind = e["kind"].as_str().unwrap_or("?");
        println!("{idx}\t{kind}\t{}", entry_summary(e));
    }
}

/// One-line digest of a log entry (a CLI display concern; the server ships only
/// the raw entry).
fn entry_summary(entry: &Value) -> String {
    let p = &entry["payload"];
    let change = entry["change_id"].as_u64().unwrap_or(0);
    match entry["kind"].as_str().unwrap_or("?") {
        "revision" => format!(
            "change {change} new revision {}",
            short_sha(p["commit_sha"].as_str().unwrap_or(""))
        ),
        "review" => format!(
            "reviewer {} on change {change} r{} ({} comment(s))",
            p["verdict"].as_str().unwrap_or("?"),
            p["revision"].as_u64().unwrap_or(0),
            p["comments"].as_array().map_or(0, Vec::len)
        ),
        "comment" => match p["thread_id"].as_u64() {
            Some(thread) => format!("agent commented on thread {thread} (change {change})"),
            None => format!("agent opened a thread on change {change}"),
        },
        "partial" => format!(
            "change {change} marked {}",
            if p["partial"].as_bool().unwrap_or(false) {
                "partial"
            } else {
                "ready"
            }
        ),
        "lifecycle" => format!("change {change} {}", p["action"].as_str().unwrap_or("?")),
        other => format!("{other} entry"),
    }
}

/// Compact one-line-per-change digest of a `Chain` for `nit status --oneline`.
fn chain_oneline(chain: &Value) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let inf = "write to String is infallible";
    writeln!(out, "state={}", chain["state"].as_str().unwrap_or("?")).expect(inf);
    let path = chain["path"].as_array().map_or(&[][..], Vec::as_slice);
    for m in path {
        writeln!(
            out,
            "{}\t{}\t{}\tr{}\t{}u\t{}",
            m["position"].as_u64().unwrap_or(0),
            short_key(m["change_key"].as_str().unwrap_or("")),
            m["status"].as_str().unwrap_or("?"),
            m["revision"].as_u64().unwrap_or(0),
            m["counts"]["unresolved"].as_u64().unwrap_or(0),
            m["subject"].as_str().unwrap_or(""),
        )
        .expect(inf);
    }
    out
}

fn short_key(key: &str) -> String {
    key.chars().take(9).collect()
}

/// Parse a `--range` spec `START-END`, each endpoint `line:char`.
fn parse_comment_range(spec: &str) -> Result<CommentRange> {
    let (start, end) = spec
        .split_once('-')
        .ok_or_else(|| anyhow!("range must be START-END (e.g. 12:4-14:7), got {spec:?}"))?;
    let point = |s: &str| -> Result<(u64, u64)> {
        let (line, ch) = s
            .split_once(':')
            .ok_or_else(|| anyhow!("range endpoint must be line:char, got {s:?}"))?;
        Ok((
            line.trim()
                .parse()
                .with_context(|| format!("bad line in {s:?}"))?,
            ch.trim()
                .parse()
                .with_context(|| format!("bad char in {s:?}"))?,
        ))
    };
    let (start_line, start_char) = point(start)?;
    let (end_line, end_char) = point(end)?;
    Ok(CommentRange {
        start_line,
        start_char,
        end_line,
        end_char,
    })
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Repo introspection (cwd → git-common-dir + HEAD)

fn repo_git_dir(path: &Path) -> Result<String> {
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

fn discover_repo() -> Result<(String, Repository)> {
    let repo = Repository::discover(".")
        .map_err(|e| anyhow!("not inside a git repository: {}", e.message()))?;
    let git_dir = git_common_dir(&repo)?;
    Ok((git_dir, repo))
}

fn head_sha(repo: &Repository) -> Result<String> {
    let head = repo.head().context("cannot resolve HEAD")?;
    let commit = head.peel_to_commit().context("HEAD is not a commit")?;
    Ok(commit.id().to_string())
}

// ---------------------------------------------------------------------------
// HTTP plumbing

#[derive(Debug)]
enum CallError {
    Unreachable(anyhow::Error),
    Fatal(anyhow::Error),
}

impl CallError {
    fn into_error(self, base: &str) -> anyhow::Error {
        match self {
            CallError::Unreachable(cause) => {
                anyhow!("cannot reach the nit server at {base}: {cause} — is 'nit serve' running?")
            }
            CallError::Fatal(err) => err,
        }
    }
}

fn classify(err: ureq::Error, path: &str) -> CallError {
    match err {
        ureq::Error::Io(_) | ureq::Error::ConnectionFailed | ureq::Error::Timeout(_) => {
            CallError::Unreachable(err.into())
        }
        ureq::Error::Json(ref e) if e.io_error_kind().is_some() => {
            CallError::Unreachable(err.into())
        }
        ureq::Error::Json(_) => {
            CallError::Fatal(anyhow::Error::new(err).context(format!("invalid JSON from {path}")))
        }
        _ => CallError::Fatal(err.into()),
    }
}

/// Classify a websocket connect/read failure. A refused or reset connection is
/// the server-restart signature and retries; `tungstenite` reports a refused
/// connect as `Error::Io` **or** `Error::Url(UnableToConnect)` — both are
/// transport, not a misconfiguration.
fn classify_ws(err: &tungstenite::Error) -> CallError {
    match err {
        tungstenite::Error::Io(_)
        | tungstenite::Error::Url(_)
        | tungstenite::Error::ConnectionClosed
        | tungstenite::Error::AlreadyClosed
        | tungstenite::Error::Protocol(_) => CallError::Unreachable(anyhow!("websocket: {err}")),
        other => CallError::Fatal(anyhow!("websocket: {other}")),
    }
}

/// Retry policy while the server is unreachable. `Fatal` errors always fail
/// immediately.
#[derive(Clone, Copy)]
enum Retry {
    /// Fail fast (push/status/comment) — an immediate "is 'nit serve' running?"
    /// beats hanging.
    No,
    /// Keep retrying with backoff (`nit wait`/`--follow` riding out a restart).
    UntilUp,
}

/// Backoff between reconnect attempts: 1, 2, 4, 8, then 10s, capped.
fn retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(1 << attempt.min(4)).min(std::time::Duration::from_secs(10))
}

/// A connected websocket to the nit server.
type WsConn = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

struct Client {
    agent: ureq::Agent,
    base: String,
}

impl Client {
    fn new(base: String) -> Self {
        let config = ureq::config::Config::builder()
            .http_status_as_error(false)
            .build();
        Client {
            agent: config.new_agent(),
            base,
        }
    }

    fn get(&self, path: &str) -> Result<Value> {
        self.get_raw(path).map_err(|e| e.into_error(&self.base))
    }

    /// Run `op`, retrying with backoff while the server is unreachable (one
    /// stderr notice per outage). `Fatal` always fails immediately; `Retry::No`
    /// fails on the first unreachable error.
    fn retry_loop<T>(
        &self,
        retry: Retry,
        mut op: impl FnMut() -> Result<T, CallError>,
    ) -> Result<T> {
        let mut attempt = 0u32;
        loop {
            let cause = match op() {
                Ok(value) => return Ok(value),
                Err(fatal @ CallError::Fatal(_)) => return Err(fatal.into_error(&self.base)),
                Err(CallError::Unreachable(cause)) => cause,
            };
            if !matches!(retry, Retry::UntilUp) {
                return Err(CallError::Unreachable(cause).into_error(&self.base));
            }
            if attempt == 0 {
                eprintln!("nit: server unreachable ({cause}); retrying…");
            }
            std::thread::sleep(retry_delay(attempt));
            attempt += 1;
        }
    }

    /// GET, retrying with backoff while the server is unreachable.
    fn get_retry(&self, path: &str, retry: Retry) -> Result<Value> {
        self.retry_loop(retry, || self.get_raw(path))
    }

    /// Connect the change stream and `subscribe` `subs` (`change_id` →
    /// from-idx), retrying the connect while the server is unreachable.
    fn ws_connect(
        &self,
        subs: &std::collections::HashMap<u64, u64>,
        retry: Retry,
    ) -> Result<WsConn> {
        let url = format!("{}/api/stream", self.base.replacen("http", "ws", 1));
        let map: std::collections::HashMap<String, u64> =
            subs.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let sub = json!({ "subscribe": map }).to_string();
        self.retry_loop(retry, || Self::try_ws(&url, &sub))
    }

    fn try_ws(url: &str, sub: &str) -> Result<WsConn, CallError> {
        let (mut socket, _) = tungstenite::connect(url).map_err(|e| classify_ws(&e))?;
        socket
            .send(tungstenite::Message::Text(sub.to_string().into()))
            .map_err(|e| classify_ws(&e))?;
        Ok(socket)
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.post_raw(path, body)
            .map_err(|e| e.into_error(&self.base))
    }

    fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        self.patch_raw(path, body)
            .map_err(|e| e.into_error(&self.base))
    }

    fn get_raw(&self, path: &str) -> Result<Value, CallError> {
        let url = format!("{}{path}", self.base);
        let response = self.agent.get(&url).call().map_err(|e| classify(e, path))?;
        Self::read(response, path)
    }

    fn post_raw(&self, path: &str, body: &Value) -> Result<Value, CallError> {
        let url = format!("{}{path}", self.base);
        let response = self
            .agent
            .post(&url)
            .send_json(body)
            .map_err(|e| classify(e, path))?;
        Self::read(response, path)
    }

    fn patch_raw(&self, path: &str, body: &Value) -> Result<Value, CallError> {
        let url = format!("{}{path}", self.base);
        let response = self
            .agent
            .patch(&url)
            .send_json(body)
            .map_err(|e| classify(e, path))?;
        Self::read(response, path)
    }

    fn read(
        mut response: ureq::http::Response<ureq::Body>,
        path: &str,
    ) -> Result<Value, CallError> {
        let status = response.status();
        let value: Value = response
            .body_mut()
            .read_json()
            .map_err(|e| classify(e, path))?;
        if !status.is_success() {
            let message = value["error"].as_str().unwrap_or("unknown error");
            return Err(CallError::Fatal(anyhow!(
                "{path}: {} — {message}",
                status.as_u16()
            )));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_range_forms_and_rejections() {
        let ok = |s: &str| LogRange::parse(s).expect("range should parse");
        assert_eq!(ok("3"), LogRange::Closed { from: 3, to: 4 });
        assert_eq!(ok("3..6"), LogRange::Closed { from: 3, to: 6 });
        assert_eq!(ok("3.."), LogRange::Open { from: 3 });
        assert_eq!(ok("..6"), LogRange::Closed { from: 0, to: 6 });
        assert_eq!(ok(".."), LogRange::Open { from: 0 });
        assert!(LogRange::parse("6..6").is_err());
        assert!(LogRange::parse("6..3").is_err());
        assert!(LogRange::parse("-1").is_err());
        assert!(LogRange::parse("notanumber").is_err());
    }

    #[test]
    fn log_range_bounds_clamp_to_len() {
        assert_eq!(LogRange::Open { from: 2 }.bounds(5), (2, 5));
        assert_eq!(LogRange::Open { from: 9 }.bounds(5), (5, 5));
        assert_eq!(LogRange::Closed { from: 1, to: 3 }.bounds(5), (1, 3));
        assert_eq!(LogRange::Closed { from: 1, to: 9 }.bounds(5), (1, 5));
        assert_eq!(LogRange::Closed { from: 9, to: 10 }.bounds(5), (5, 5));
    }

    #[test]
    fn entry_summary_digests_each_kind() {
        let rev = json!({"change_id": 7, "kind": "revision", "payload": {"commit_sha": "abcdef0123456789"}});
        assert_eq!(entry_summary(&rev), "change 7 new revision abcdef012345");
        let review = json!({"change_id": 7, "kind": "review",
            "payload": {"verdict": "request_changes", "revision": 2, "comments": [{}, {}]}});
        assert_eq!(
            entry_summary(&review),
            "reviewer request_changes on change 7 r2 (2 comment(s))"
        );
        let opened = json!({"change_id": 7, "kind": "comment", "payload": {"thread_id": null}});
        assert_eq!(entry_summary(&opened), "agent opened a thread on change 7");
        let life = json!({"change_id": 7, "kind": "lifecycle", "payload": {"action": "merged"}});
        assert_eq!(entry_summary(&life), "change 7 merged");
    }

    #[test]
    fn parse_comment_range_forms_and_rejections() {
        assert_eq!(
            parse_comment_range("12:4-14:7").expect("ok"),
            CommentRange {
                start_line: 12,
                start_char: 4,
                end_line: 14,
                end_char: 7,
            }
        );
        assert!(parse_comment_range("12:4").is_err());
        assert!(parse_comment_range("12-14").is_err());
        assert!(parse_comment_range("a:4-14:7").is_err());
    }

    #[test]
    fn chain_oneline_digests_each_member() {
        let chain = json!({
            "state": "agents_turn",
            "path": [
                {"position": 0, "change_key": "I0123456789abc", "status": "changes_requested",
                 "revision": 2, "counts": {"unresolved": 3}, "subject": "server: add health endpoint"},
                {"position": 1, "change_key": "Iabcdef0123456", "status": "approved",
                 "revision": 1, "counts": {"unresolved": 0}, "subject": "web: render the diff"},
            ]
        });
        assert_eq!(
            chain_oneline(&chain),
            "state=agents_turn\n\
             0\tI01234567\tchanges_requested\tr2\t3u\tserver: add health endpoint\n\
             1\tIabcdef01\tapproved\tr1\t0u\tweb: render the diff\n"
        );
    }

    #[test]
    fn server_url_resolution_order() {
        assert_eq!(
            server_url(Some("http://x:1".into())),
            "http://x:1".to_string()
        );
        if std::env::var("NIT_SERVER").is_err() {
            assert_eq!(server_url(None), DEFAULT_SERVER.to_string());
        }
    }

    #[test]
    fn agent_echoes_are_the_agents_own_writes() {
        let echo = |kind: &str| is_agent_echo(&json!({"kind": kind, "payload": {}}));
        assert!(echo("revision"));
        assert!(echo("comment"));
        assert!(echo("partial"));
        // Reviewer activity and lifecycle always reach a --reviewer-only monitor.
        assert!(!echo("review"));
        assert!(!echo("lifecycle"));
        // Unrecognized kinds fail open — relayed, never hidden.
        assert!(!echo("some_future_kind"));
    }

    #[test]
    fn follow_cursor_forms() {
        assert_eq!(follow_cursor("0").expect("zero"), 0);
        assert_eq!(follow_cursor("5").expect("five"), 5);
        assert_eq!(follow_cursor("5..").expect("open"), 5);
        assert_eq!(follow_cursor("..").expect("all"), 0);
        assert!(follow_cursor("nope").is_err());
    }
}
