//! `nit push` / `wait` / `status` / `reply` — thin CLI clients of the
//! HTTP API, run by coding agents from inside a git repo
//! (docs/agent-workflow.md). They print API JSON to stdout and decide
//! purely on the documented shapes; all review logic lives server-side.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use git2::Repository;
use serde_json::{Value, json};

pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8877";

fn server_url(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("NIT_SERVER").ok())
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

// ---------------------------------------------------------------------------
// Args

#[derive(clap::Args)]
pub struct PushArgs {
    /// Base ref to review against (default: main, falling back to master)
    #[arg(long)]
    pub base: Option<String>,
    /// Branch to register (default: the current HEAD branch)
    #[arg(long)]
    pub branch: Option<String>,
    /// Mark the chain partial: review can start, merging
    /// cannot; sticky until `nit ready`
    #[arg(long)]
    pub partial: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct ReadyArgs {
    /// Base ref to review against (default: main, falling back to master)
    #[arg(long)]
    pub base: Option<String>,
    /// Branch to register (default: the current HEAD branch)
    #[arg(long)]
    pub branch: Option<String>,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct WaitArgs {
    /// 0-based cursor: the count of log entries already consumed (start at 0,
    /// then pass the `head` of each result; docs/agent-workflow.md)
    pub cursor: u64,
    /// Print a one-line digest per entry instead of full payloads
    #[arg(long)]
    pub oneline: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct LogArgs {
    /// Entry indices or half-open ranges: `3`, `5..9`, `5..` (through head),
    /// `..9`, `..` (all). Several are concatenated in order, e.g. `1 4..6`.
    #[arg(required = true)]
    pub ranges: Vec<String>,
    /// Print a one-line digest per entry instead of full payloads
    #[arg(long)]
    pub oneline: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct StatusArgs {
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct ReplyArgs {
    /// Id of the comment to reply to
    pub comment_id: u64,
    /// Reply text
    #[arg(short = 'm', long = "message")]
    pub message: String,
    /// Mark the thread resolved
    #[arg(long)]
    pub resolve: bool,
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

// ---------------------------------------------------------------------------
// Commands

/// Register/refresh the current branch as a chain; idempotent.
/// `--partial` marks the chain partial; without it the sticky flag is left
/// untouched (never cleared by a plain push).
///
/// # Errors
/// When the repo or server is unreachable, and when the scan failed —
/// the chain JSON still prints first so the agent sees
/// `last_scan_error` and `web_url`.
pub fn push(args: PushArgs) -> Result<()> {
    register(
        args.base,
        args.branch,
        args.server,
        args.partial.then_some(true),
    )
}

/// Mark the chain complete: clear the sticky partial flag set by
/// `nit push --partial` and refresh; idempotent.
///
/// # Errors
/// Same as [`push`].
pub fn ready(args: ReadyArgs) -> Result<()> {
    register(args.base, args.branch, args.server, Some(false))
}

/// Shared push/ready core: register/refresh the chain via
/// `POST /api/chains`, sending `partial` only when an override is given
/// (absent leaves the server's sticky flag unchanged).
fn register(
    base: Option<String>,
    branch: Option<String>,
    server: Option<String>,
    partial: Option<bool>,
) -> Result<()> {
    let (root, repo) = discover_repo()?;
    let branch = match branch {
        Some(b) => b,
        None => current_branch(&repo)?,
    };
    let base = match base {
        Some(b) => b,
        None => default_base(&repo)?,
    };
    let client = Client::new(server_url(server));
    let mut body = json!({
        "repo_path": root.to_string_lossy(),
        "branch": branch,
        "base": base,
    });
    if let Some(partial) = partial {
        body["partial"] = json!(partial);
    }
    let chain = client.post("/api/chains", &body)?;
    print_json(&chain)?;
    if let Some(err) = chain["last_scan_error"].as_str() {
        bail!("scan failed: {err}");
    }
    Ok(())
}

/// Consume the chain's `/events` SSE stream from the agent-owned `cursor`
/// and block until something the agent should act on lands, then print
/// `{head, entries, feedback}`. There is no timeout — the agent calls this
/// only when it has nothing else to do, and it blocks until the reviewer
/// acts. The wake rule lives here, not on the server (docs/data-model.md):
/// every event wakes **except** a reviewer approve with no comments that
/// does not complete the chain — those are accumulated and handed back with
/// the next waking event, never dropped. The agent advances its cursor to
/// the returned `head`; it never learns the cursor from a mutating call, so
/// an interleaved reviewer entry can't be skipped (docs/agent-workflow.md).
/// Rides out server restarts: the stream reconnects at the current cursor.
///
/// # Errors
/// When the server returns an error or a malformed response.
pub fn wait(args: WaitArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let retry = Retry::UntilUp { deadline: None };
    let chain_id = resolve_chain(&client, retry)?;
    let mut cursor = args.cursor;
    // Accumulated since `cursor` — so a return carries the complete run,
    // including any non-waking approves we read past.
    let mut entries: Vec<Value> = Vec::new();

    'reconnect: loop {
        let mut stream = client.get_stream(
            &format!("/api/chains/{chain_id}/events?cursor={cursor}"),
            retry,
        )?;
        loop {
            // End of stream (graceful shutdown) or a severed connection:
            // reconnect at the cursor we have reached.
            let Ok(Some(data)) = next_sse_data(&mut stream) else {
                continue 'reconnect;
            };
            // A malformed frame is a protocol violation the server never
            // emits; fail loudly rather than silently drop the event.
            let entry: Value = serde_json::from_str(&data)
                .with_context(|| format!("malformed event from the server: {data:?}"))?;
            cursor = entry["idx"].as_u64().map_or(cursor, |i| i + 1);
            entries.push(entry.clone());

            // A pure approve wakes only when it *completes* the chain, so we
            // need the resulting state for that case (and only that case);
            // every other event wakes unconditionally.
            let feedback = if is_pure_approve(&entry) {
                Some(client.get(&format!("/api/chains/{chain_id}/feedback"))?)
            } else {
                None
            };
            let state = feedback
                .as_ref()
                .and_then(|fb| fb["state"].as_str())
                .unwrap_or("");
            if event_wakes(&entry, state) {
                let feedback = match feedback {
                    Some(fb) => fb,
                    None => client.get(&format!("/api/chains/{chain_id}/feedback"))?,
                };
                let resp = json!({"head": cursor, "entries": entries, "feedback": feedback});
                print_wait(&resp, args.oneline)?;
                return Ok(());
            }
            // A non-completing pure approve: keep it accumulated and keep
            // reading — it never surfaces a parked wait on its own.
        }
    }
}

/// Whether one streamed event should end a parked `nit wait`, given the
/// chain's resulting `feedback.state`. Every event wakes **except** a
/// reviewer approve with no comments that did not complete the chain (left
/// it short of `ready_to_merge`) — those accumulate silently until a waking
/// event arrives. `state` is only consulted for that suppressed case.
fn event_wakes(entry: &Value, state: &str) -> bool {
    !is_pure_approve(entry) || state == "ready_to_merge"
}

/// A reviewer `approve` with no comments — the only event kind that can be
/// suppressed (see [`event_wakes`]).
fn is_pure_approve(entry: &Value) -> bool {
    entry["kind"] == "review"
        && entry["payload"]["verdict"] == "approve"
        && entry["payload"]["comments"]
            .as_array()
            .is_none_or(Vec::is_empty)
}

/// Read the next SSE `data:` event from `reader`, joining multi-line data.
/// Returns `Ok(None)` at end of stream (the server closed). Keep-alive
/// comment lines (`:`) and non-`data` fields are skipped.
fn next_sse_data<R: BufRead>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut data = String::new();
    let mut have = false;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if have {
                return Ok(Some(data));
            }
            continue;
        }
        if let Some(rest) = l.strip_prefix("data:") {
            if have {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            have = true;
        }
    }
}

/// Print specific log entries by index/range without moving any cursor.
///
/// # Errors
/// When a range is malformed or the server can't be reached.
pub fn log(args: LogArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let chain_id = resolve_chain(&client, Retry::No)?;
    let mut entries: Vec<Value> = Vec::new();
    let mut head = 0;
    for spec in &args.ranges {
        // Each token is fetched independently and concatenated in order,
        // duplicates kept — `nit log 1..3 1..3` returns 1,2,1,2.
        let url = match LogRange::parse(spec)?.query() {
            (from, Some(to)) => format!("/api/chains/{chain_id}/log?from={from}&to={to}"),
            (from, None) => format!("/api/chains/{chain_id}/log?from={from}"),
        };
        let resp = client.get(&url)?;
        head = resp["head"].as_u64().unwrap_or(head);
        if let Some(arr) = resp["entries"].as_array() {
            entries.extend(arr.iter().cloned());
        }
    }
    if args.oneline {
        println!("head={head}");
        print_oneline_entries(&entries);
    } else {
        print_json(&json!({"head": head, "entries": entries}))?;
    }
    Ok(())
}

/// A parsed `nit log` selector, half-open and unsigned. Built only via
/// [`LogRange::parse`], which rejects reverse/empty ranges — so a `Closed`
/// always has `to > from`, and an illegal range can't be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogRange {
    /// `A..` / `..` — `[from, head)`, through the current head.
    Open { from: u64 },
    /// `A` / `A..B` / `..B` — `[from, to)` with `to > from`.
    Closed { from: u64, to: u64 },
}

impl LogRange {
    /// Parse one `nit log` token: `A` (the single entry `A`), `A..B`, `A..`,
    /// `..B`, or `..` (all half-open). An empty side defaults to `0` (start)
    /// or "through head" (end).
    fn parse(spec: &str) -> Result<LogRange> {
        let num = |s: &str| -> Result<u64> {
            s.trim()
                .parse::<u64>()
                .with_context(|| format!("bad index {:?}", s.trim()))
        };
        let Some((a, b)) = spec.split_once("..") else {
            // A bare index `A` selects exactly `[A, A+1)`.
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

    /// The `from`/`to` query params for `/log` (`to = None` ⇒ open through
    /// head).
    fn query(self) -> (u64, Option<u64>) {
        match self {
            LogRange::Open { from } => (from, None),
            LogRange::Closed { from, to } => (from, Some(to)),
        }
    }
}

fn print_wait(resp: &Value, oneline: bool) -> Result<()> {
    if !oneline {
        return print_json(resp);
    }
    let head = resp["head"].as_u64().unwrap_or(0);
    let state = resp["feedback"]["state"].as_str().unwrap_or("?");
    println!("head={head} state={state}");
    if let Some(arr) = resp["entries"].as_array() {
        print_oneline_entries(arr);
    }
    Ok(())
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

/// One-line digest of a log entry for `--oneline`. This is a CLI display
/// concern: the server ships only the raw entry (idx/kind/payload), and the
/// digest is derived here on demand, so its wording can change freely
/// without an API change (docs/api.md `LogEntry`).
fn entry_summary(entry: &Value) -> String {
    let p = &entry["payload"];
    match entry["kind"].as_str().unwrap_or("?") {
        "revisions" => {
            let added = p["added"].as_array().map_or(&[][..], Vec::as_slice);
            if added.is_empty() {
                let live = p["live"].as_array().map_or(0, Vec::len);
                format!("scan: {live} live change(s)")
            } else {
                let keys: Vec<String> = added
                    .iter()
                    .map(|a| {
                        format!(
                            "{} r{}",
                            short_key(a["change_key"].as_str().unwrap_or("")),
                            a["number"].as_u64().unwrap_or(0)
                        )
                    })
                    .collect();
                format!("push: {}", keys.join(", "))
            }
        }
        "review" => format!(
            "reviewer {} {} r{} ({} comment(s))",
            p["verdict"].as_str().unwrap_or("?"),
            short_key(p["change_key"].as_str().unwrap_or("")),
            p["revision"].as_u64().unwrap_or(0),
            p["comments"].as_array().map_or(0, Vec::len)
        ),
        "reply" => format!(
            "agent replied to {} comment(s)",
            p["replies"].as_array().map_or(0, Vec::len)
        ),
        "resolve" => {
            let verb = if p["resolved"].as_bool().unwrap_or(true) {
                "resolved"
            } else {
                "unresolved"
            };
            format!("reviewer {verb} a thread")
        }
        "partial" => format!(
            "chain marked {}",
            if p["partial"].as_bool().unwrap_or(false) {
                "partial"
            } else {
                "ready"
            }
        ),
        "chain_closed" => format!("chain {}", p["status"].as_str().unwrap_or("closed")),
        other => format!("{other} entry"),
    }
}

fn short_key(key: &str) -> String {
    key.chars().take(9).collect()
}

/// Print the current Feedback JSON without blocking.
///
/// # Errors
/// When the server can't be reached or no chain matches the current
/// branch.
pub fn status(args: StatusArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let chain_id = resolve_chain(&client, Retry::No)?;
    let feedback = client.get(&format!("/api/chains/{chain_id}/feedback"))?;
    print_json(&feedback)
}

/// Threaded reply as the agent; `--resolve` closes the thread.
///
/// # Errors
/// When the server can't be reached or the comment id is unknown.
pub fn reply(args: ReplyArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let comment = client.post(
        &format!("/api/comments/{}/replies", args.comment_id),
        &json!({"body": args.message, "resolve": args.resolve}),
    )?;
    print_json(&comment)
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Repo introspection (cwd → repo root, branch, base)

fn discover_repo() -> Result<(PathBuf, Repository)> {
    let repo = Repository::discover(".")
        .map_err(|e| anyhow!("not inside a git repository: {}", e.message()))?;
    let root = repo
        .workdir()
        .ok_or_else(|| anyhow!("bare repositories are not supported"))?
        .to_path_buf();
    Ok((root, repo))
}

fn current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head().context("cannot resolve HEAD")?;
    if !head.is_branch() {
        bail!("HEAD is detached — check out a branch or pass --branch");
    }
    head.shorthand()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("branch name is not valid UTF-8"))
}

fn default_base(repo: &Repository) -> Result<String> {
    for candidate in ["main", "master"] {
        if repo.revparse_single(candidate).is_ok() {
            return Ok(candidate.to_string());
        }
    }
    bail!("neither 'main' nor 'master' exists — pass --base");
}

/// The registered chain for the cwd's repo + branch, via
/// `GET /api/chains?status=all` (the server stores canonicalized paths).
/// `retry` covers only that GET — repo discovery and "branch not
/// registered" stay fatal.
fn resolve_chain(client: &Client, retry: Retry) -> Result<u64> {
    let (root, repo) = discover_repo()?;
    let branch = current_branch(&repo)?;
    let canonical = std::fs::canonicalize(&root)
        .with_context(|| format!("cannot resolve repo path {}", root.display()))?;
    let list = client.get_retry("/api/chains?status=all", retry)?;
    let chains = list["chains"]
        .as_array()
        .ok_or_else(|| anyhow!("malformed chain list: {list}"))?;
    chains
        .iter()
        .find(|c| {
            c["repo_path"].as_str() == canonical.to_str() && c["branch"].as_str() == Some(&branch)
        })
        .and_then(|c| c["id"].as_u64())
        .ok_or_else(|| {
            anyhow!("branch '{branch}' is not registered with nit — run 'nit push' first")
        })
}

// ---------------------------------------------------------------------------
// HTTP plumbing

/// A failed call, classified for retry decisions: is the server merely
/// unreachable (down/restarting — a retry can succeed) or did it answer
/// definitively (HTTP error body, malformed response, misconfiguration)?
#[derive(Debug)]
enum CallError {
    /// Transport-level failure: nobody answered, or the connection died
    /// mid-response.
    Unreachable(anyhow::Error),
    /// A definitive failure retrying cannot fix.
    Fatal(anyhow::Error),
}

impl CallError {
    /// Today's user-facing error: `Unreachable` keeps the exact
    /// "is 'nit serve' running?" message (`cli_e2e` asserts it).
    fn into_error(self, base: &str) -> anyhow::Error {
        match self {
            CallError::Unreachable(cause) => {
                anyhow!("cannot reach the nit server at {base}: {cause} — is 'nit serve' running?")
            }
            CallError::Fatal(err) => err,
        }
    }
}

/// Classify a ureq failure. With `http_status_as_error(false)` every
/// `Err` here is transport-or-client-side — HTTP error bodies arrive as
/// non-2xx *responses* and become `Fatal` in [`Client::read`] instead.
fn classify(err: ureq::Error, path: &str) -> CallError {
    match err {
        // Refused/reset connections and timeouts: the restart signature.
        ureq::Error::Io(_) | ureq::Error::ConnectionFailed | ureq::Error::Timeout(_) => {
            CallError::Unreachable(err.into())
        }
        // read_json wraps body io errors in serde_json: io-kind means the
        // server died mid-body, anything else is a malformed response.
        ureq::Error::Json(ref e) if e.io_error_kind().is_some() => {
            CallError::Unreachable(err.into())
        }
        ureq::Error::Json(_) => {
            CallError::Fatal(anyhow::Error::new(err).context(format!("invalid JSON from {path}")))
        }
        // BadUri, HostNotFound, Tls, Protocol, …: persistent
        // misconfiguration or protocol trouble — fail fast.
        _ => CallError::Fatal(err.into()),
    }
}

/// What to do when the server is unreachable ([`CallError::Fatal`]
/// always fails immediately).
#[derive(Clone, Copy)]
enum Retry {
    /// Fail fast — push/status/reply, where an immediate "is 'nit serve'
    /// running?" beats hanging and rerunning is cheap.
    No,
    /// Keep retrying with backoff until the server is back (`None`
    /// deadline: forever) — `nit wait` riding out a server restart.
    UntilUp { deadline: Option<Instant> },
}

/// Backoff between reconnect attempts: 1, 2, 4, 8 then 10s, capped.
fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(1 << attempt.min(4)).min(Duration::from_secs(10))
}

struct Client {
    agent: ureq::Agent,
    base: String,
}

impl Client {
    fn new(base: String) -> Self {
        // Non-2xx must reach us as bodies ({"error": …}), not transport
        // errors.
        let config = ureq::config::Config::builder()
            .http_status_as_error(false)
            .build();
        Client {
            agent: config.new_agent(),
            base,
        }
    }

    fn get(&self, path: &str) -> Result<Value> {
        self.get_retry(path, Retry::No)
    }

    /// GET with `Retry` semantics while the server is unreachable. One
    /// stderr notice per outage (an outage is contained in a single call:
    /// the next call only starts after a success); stdout stays pure
    /// JSON. When a deadline is set, sleeps are capped at the remaining
    /// time and its expiry returns the transport error instead of a
    /// stale snapshot — exit 0 must mean the feedback is fresh.
    fn get_retry(&self, path: &str, retry: Retry) -> Result<Value> {
        let mut attempt: u32 = 0;
        loop {
            let cause = match self.get_raw(path) {
                Ok(value) => return Ok(value),
                Err(fatal @ CallError::Fatal(_)) => return Err(fatal.into_error(&self.base)),
                Err(CallError::Unreachable(cause)) => cause,
            };
            let Retry::UntilUp { deadline } = retry else {
                return Err(CallError::Unreachable(cause).into_error(&self.base));
            };
            if attempt == 0 {
                eprintln!("nit: server unreachable ({cause}); retrying…");
            }
            let mut delay = retry_delay(attempt);
            if let Some(d) = deadline {
                let remaining = d.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(CallError::Unreachable(cause)
                        .into_error(&self.base)
                        .context(
                            "gave up: --timeout expired while the nit server was unreachable",
                        ));
                }
                delay = delay.min(remaining);
            }
            std::thread::sleep(delay);
            attempt += 1;
        }
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.post_raw(path, body)
            .map_err(|e| e.into_error(&self.base))
    }

    /// Open a streaming GET for the SSE `/events` endpoint. Retries the
    /// *connect* with backoff while the server is unreachable (`Retry::No`
    /// fails fast); the returned reader then streams the body. One stderr
    /// notice per outage, matching [`Client::get_retry`].
    fn get_stream(&self, path: &str, retry: Retry) -> Result<impl BufRead + use<>> {
        let url = format!("{}{path}", self.base);
        let mut attempt: u32 = 0;
        loop {
            let cause = match self.agent.get(&url).call() {
                Ok(resp) => return Ok(BufReader::new(resp.into_body().into_reader())),
                Err(e) => match classify(e, path) {
                    fatal @ CallError::Fatal(_) => return Err(fatal.into_error(&self.base)),
                    CallError::Unreachable(cause) => cause,
                },
            };
            if !matches!(retry, Retry::UntilUp { .. }) {
                return Err(CallError::Unreachable(cause).into_error(&self.base));
            }
            if attempt == 0 {
                eprintln!("nit: server unreachable ({cause}); retrying…");
            }
            std::thread::sleep(retry_delay(attempt));
            attempt += 1;
        }
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
    use git2::{RepositoryInitOptions, Signature, Time};

    fn repo_with_head(initial_head: &str) -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head(initial_head);
        let repo = Repository::init_opts(dir.path(), &opts).expect("test repo should init");
        let sig =
            Signature::new("t", "t@example.com", &Time::new(0, 0)).expect("signature should build");
        let tree_oid = repo
            .treebuilder(None)
            .expect("treebuilder should create")
            .write()
            .expect("tree should write");
        {
            let tree = repo.find_tree(tree_oid).expect("tree should exist");
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .expect("commit should create");
        }
        (dir, repo)
    }

    #[test]
    fn current_branch_from_head() {
        let (_dir, repo) = repo_with_head("refs/heads/feat/x");
        assert_eq!(
            current_branch(&repo).expect("branch should resolve"),
            "feat/x"
        );
    }

    #[test]
    fn log_range_forms_and_rejections() {
        let ok = |s: &str| LogRange::parse(s).expect("range should parse");
        assert_eq!(ok("3"), LogRange::Closed { from: 3, to: 4 });
        assert_eq!(ok("3..6"), LogRange::Closed { from: 3, to: 6 });
        assert_eq!(ok("3.."), LogRange::Open { from: 3 });
        assert_eq!(ok("..6"), LogRange::Closed { from: 0, to: 6 });
        assert_eq!(ok(".."), LogRange::Open { from: 0 });
        // Reverse / empty closed ranges are rejected, not silently emptied.
        assert!(LogRange::parse("6..6").is_err());
        assert!(LogRange::parse("6..3").is_err());
        // A bare u64::MAX overflows the +1: a clean error, not a panic.
        assert!(LogRange::parse(&format!("{}", u64::MAX)).is_err());
        // Negatives never parse as an unsigned index.
        assert!(LogRange::parse("-1").is_err());
        assert!(LogRange::parse("notanumber").is_err());
    }

    #[test]
    fn event_wakes_only_on_completing_pure_approve() {
        let approve = |comments: Value| json!({"kind": "review", "payload": {"verdict": "approve", "comments": comments}});
        // A pure approve wakes only when it completes the chain — NOT on a
        // merely-actionable state (e.g. all-approved-while-partial is
        // `agents_turn`, not `ready_to_merge`).
        assert!(!event_wakes(&approve(json!([])), "agents_turn"));
        assert!(!event_wakes(&approve(json!([])), "waiting_for_review"));
        assert!(event_wakes(&approve(json!([])), "ready_to_merge"));
        // An approve with comments wakes regardless of state.
        assert!(event_wakes(
            &approve(json!([{"body": "nit"}])),
            "agents_turn"
        ));
        // Every non-(pure-approve) event wakes unconditionally.
        let request =
            json!({"kind": "review", "payload": {"verdict": "request_changes", "comments": []}});
        assert!(event_wakes(&request, "agents_turn"));
        assert!(event_wakes(
            &json!({"kind": "revisions", "payload": {}}),
            "waiting_for_review"
        ));
        assert!(event_wakes(
            &json!({"kind": "reply", "payload": {}}),
            "waiting_for_review"
        ));
    }

    #[test]
    fn entry_summary_digests_each_kind() {
        let push = json!({"kind": "revisions", "payload":
            {"live": [{}], "added": [{"change_key": "I0123456789abc", "number": 2}]}});
        assert_eq!(entry_summary(&push), "push: I01234567 r2");
        let scan = json!({"kind": "revisions", "payload": {"live": [{}, {}], "added": []}});
        assert_eq!(entry_summary(&scan), "scan: 2 live change(s)");
        let review = json!({"kind": "review", "payload":
            {"verdict": "request_changes", "change_key": "I0123456789", "revision": 2, "comments": [{}, {}]}});
        assert_eq!(
            entry_summary(&review),
            "reviewer request_changes I01234567 r2 (2 comment(s))"
        );
        let resolve = json!({"kind": "resolve", "payload": {"resolved": false}});
        assert_eq!(entry_summary(&resolve), "reviewer unresolved a thread");
        let closed = json!({"kind": "chain_closed", "payload": {"status": "merged"}});
        assert_eq!(entry_summary(&closed), "chain merged");
    }

    #[test]
    fn current_branch_rejects_detached_head() {
        let (_dir, repo) = repo_with_head("refs/heads/main");
        let oid = repo
            .head()
            .expect("HEAD should resolve")
            .target()
            .expect("HEAD should point at a commit");
        repo.set_head_detached(oid).expect("detach should succeed");
        assert!(current_branch(&repo).is_err());
    }

    #[test]
    fn default_base_prefers_main_then_master() {
        let (_dir, repo) = repo_with_head("refs/heads/main");
        assert_eq!(default_base(&repo).expect("base should resolve"), "main");

        let (_dir2, repo2) = repo_with_head("refs/heads/master");
        assert_eq!(default_base(&repo2).expect("base should resolve"), "master");

        let (_dir3, repo3) = repo_with_head("refs/heads/trunk");
        assert!(default_base(&repo3).is_err());
    }

    #[test]
    fn retry_delay_backs_off_to_a_ten_second_cap() {
        let schedule: Vec<u64> = (0..6).map(|a| retry_delay(a).as_secs()).collect();
        assert_eq!(schedule, [1, 2, 4, 8, 10, 10]);
    }

    #[test]
    fn classify_transport_failures_as_unreachable() {
        let cases = [
            ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
            ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset)),
            ureq::Error::ConnectionFailed,
            ureq::Error::Timeout(ureq::Timeout::Connect),
        ];
        for err in cases {
            let label = format!("{err}");
            assert!(
                matches!(classify(err, "/x"), CallError::Unreachable(_)),
                "{label}"
            );
        }
    }

    #[test]
    fn classify_severed_response_body_as_unreachable() {
        // The server died mid-body: serde_json wraps the io error.
        struct FailingReader;
        impl std::io::Read for FailingReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
            }
        }
        let json_io = serde_json::from_reader::<_, Value>(FailingReader)
            .expect_err("reading from a failing reader must error");
        assert!(json_io.io_error_kind().is_some());
        assert!(matches!(
            classify(ureq::Error::Json(json_io), "/x"),
            CallError::Unreachable(_)
        ));
    }

    #[test]
    fn classify_definitive_failures_as_fatal() {
        let bad_uri = ureq::Error::BadUri("not a uri".into());
        assert!(matches!(classify(bad_uri, "/x"), CallError::Fatal(_)));

        let parse =
            serde_json::from_str::<Value>("not json").expect_err("parsing garbage must error");
        assert!(parse.io_error_kind().is_none());
        let classified = classify(ureq::Error::Json(parse), "/x");
        let CallError::Fatal(err) = classified else {
            panic!("JSON parse errors must be fatal");
        };
        assert!(
            format!("{err:#}").contains("invalid JSON from /x"),
            "{err:#}"
        );
    }

    #[test]
    fn server_url_resolution_order() {
        // Flag wins; the env fallback is exercised implicitly (reading a
        // process-global env var in tests would race other tests).
        assert_eq!(
            server_url(Some("http://x:1".into())),
            "http://x:1".to_string()
        );
        if std::env::var("NIT_SERVER").is_err() {
            assert_eq!(server_url(None), DEFAULT_SERVER.to_string());
        }
    }
}
