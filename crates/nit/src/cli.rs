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
/// `feedback.state`/`actionable`, never on raw events.
///
/// # Errors
/// When the server can't be reached or returns a malformed response
/// (a `--timeout` expiry prints the snapshot and exits 0).
pub fn wait(args: WaitArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let chain_id = resolve_chain(&client)?;
    let deadline = args
        .timeout
        .map(|secs| Instant::now() + Duration::from_secs(secs));

    // Bootstrap: cursor=0 returns the current snapshot immediately.
    let mut resp = client.get(&format!("/api/chains/{chain_id}/wait?cursor=0"))?;
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
        resp = client.get(&format!(
            "/api/chains/{chain_id}/wait?cursor={cursor}&timeout={poll}"
        ))?;
    }
}

/// Print the current Feedback JSON without blocking.
///
/// # Errors
/// When the server can't be reached or no chain matches the current
/// branch.
pub fn status(args: StatusArgs) -> Result<()> {
    let client = Client::new(server_url(args.server));
    let chain_id = resolve_chain(&client)?;
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
fn resolve_chain(client: &Client) -> Result<i64> {
    let (root, repo) = discover_repo()?;
    let branch = current_branch(&repo)?;
    let canonical = std::fs::canonicalize(&root)
        .with_context(|| format!("cannot resolve repo path {}", root.display()))?;
    let list = client.get("/api/chains?status=all")?;
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
        let url = format!("{}{path}", self.base);
        let response = self.agent.get(&url).call().map_err(|e| self.io_err(&e))?;
        Self::read(response, path)
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{}{path}", self.base);
        let response = self
            .agent
            .post(&url)
            .send_json(body)
            .map_err(|e| self.io_err(&e))?;
        Self::read(response, path)
    }

    fn io_err(&self, err: &ureq::Error) -> anyhow::Error {
        anyhow!(
            "cannot reach the nit server at {}: {err} — is 'nit serve' running?",
            self.base
        )
    }

    fn read(mut response: ureq::http::Response<ureq::Body>, path: &str) -> Result<Value> {
        let status = response.status();
        let value: Value = response
            .body_mut()
            .read_json()
            .with_context(|| format!("invalid JSON from {path}"))?;
        if !status.is_success() {
            let message = value["error"].as_str().unwrap_or("unknown error");
            bail!("{path}: {} — {message}", status.as_u16());
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
