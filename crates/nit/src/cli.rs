//! `nit push` / `wait` / `status` / `reply` — thin CLI clients of the
//! HTTP API, run by coding agents from inside a git repo
//! (docs/agent-workflow.md). They print API JSON to stdout and decide
//! purely on the documented shapes; all review logic lives server-side.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use git2::Repository;
use serde_json::{Value, json};

pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8877";

/// Per-poll server timeout for `nit wait` (api.md default).
const POLL_TIMEOUT_SECS: u64 = 55;

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
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`)
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(clap::Args)]
pub struct WaitArgs {
    /// Give up after this many seconds (default: wait forever)
    #[arg(long)]
    pub timeout: Option<u64>,
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
    pub comment_id: i64,
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
///
/// # Errors
/// When the repo or server is unreachable, and when the scan failed —
/// the chain JSON still prints first so the agent sees
/// `last_scan_error` and `web_url`.
pub fn push(args: PushArgs) -> Result<()> {
    let (root, repo) = discover_repo()?;
    let branch = match args.branch {
        Some(b) => b,
        None => current_branch(&repo)?,
    };
    let base = match args.base {
        Some(b) => b,
        None => default_base(&repo)?,
    };
    let client = Client::new(server_url(args.server));
    let chain = client.post(
        "/api/chains",
        &json!({
            "repo_path": root.to_string_lossy(),
            "branch": branch,
            "base": base,
        }),
    )?;
    print_json(&chain)?;
    if let Some(err) = chain["last_scan_error"].as_str() {
        bail!("scan failed: {err}");
    }
    Ok(())
}

/// Block until the chain state is actionable (or `--timeout` expires),
/// then print the Feedback JSON. Decides purely on
/// `feedback.state`/`actionable`, never on raw events. Rides out server
/// restarts: transport failures are retried with backoff (one stderr
/// notice per outage) because the wait cursor is persisted server-side.
///
/// # Errors
/// When the server returns an error or malformed response, and when
/// `--timeout` expires while the server is unreachable (a fresh snapshot
/// cannot be fetched; a plain expiry prints the snapshot and exits 0).
pub fn wait(args: WaitArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    // Computed first: --timeout also bounds the resolve phase.
    let deadline = args
        .timeout
        .map(|secs| Instant::now() + Duration::from_secs(secs));
    let retry = Retry::UntilUp { deadline };
    let chain_id = resolve_chain(&client, retry)?;

    // Bootstrap: cursor=0 returns the current snapshot immediately.
    let mut resp = client.get_retry(&format!("/api/chains/{chain_id}/wait?cursor=0"), retry)?;
    loop {
        let feedback = resp
            .get("feedback")
            .cloned()
            .ok_or_else(|| anyhow!("malformed wait response: {resp}"))?;
        let actionable = feedback["actionable"].as_bool().unwrap_or(false);
        let expired = deadline.is_some_and(|d| Instant::now() >= d);
        if actionable || expired {
            print_json(&feedback)?;
            return Ok(());
        }
        let mut poll = POLL_TIMEOUT_SECS;
        if let Some(d) = deadline {
            let remaining = d.saturating_duration_since(Instant::now()).as_secs();
            poll = poll.min(remaining.max(1));
        }
        let cursor = resp["cursor"].as_i64().unwrap_or(0);
        resp = client.get_retry(
            &format!("/api/chains/{chain_id}/wait?cursor={cursor}&timeout={poll}"),
            retry,
        )?;
    }
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
fn resolve_chain(client: &Client, retry: Retry) -> Result<i64> {
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
        .and_then(|c| c["id"].as_i64())
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
