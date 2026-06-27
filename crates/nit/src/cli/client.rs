//! HTTP/websocket transport for the CLI: the [`Client`] over the nit server's
//! JSON API, the unreachable-vs-fatal [`CallError`] split, the [`Retry`] policy
//! that rides out server restarts, and the shared `print_json`/`server_url`
//! helpers. Every request and response is a typed `nit-types` shape — no
//! `serde_json::Value` crosses this boundary.

use anyhow::{Result, anyhow};
use nit_types::error::ApiError;
use nit_types::events::ClientMsg;
use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) const DEFAULT_SERVER: &str = "http://127.0.0.1:8877";

/// The `--server` override, flattened into every command so the flag's name,
/// help, and default live in one place. `global` lets it sit before or after a
/// subcommand (it carries `nit repo`'s parent flag down to `create`/`move`);
/// on a leaf command it is a harmless no-op.
#[derive(clap::Args)]
pub struct ServerOpt {
    /// nit server URL (default: `$NIT_SERVER` or `http://127.0.0.1:8877`).
    #[arg(long, global = true)]
    pub server: Option<String>,
}

pub(crate) fn server_url(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("NIT_SERVER").ok())
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

/// Probe the server's reported build with a hard 1-second budget — the
/// reachability half of `nit --version`. Any timeout, transport, or parse
/// failure reads as unreachable (`None`); a healthy server returns its
/// `/api/health` `version`.
pub(crate) fn server_version(base: &str) -> Option<String> {
    let agent = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(1)))
        .build()
        .new_agent();
    let mut resp = agent.get(format!("{base}/api/health")).call().ok()?;
    let health: nit_types::health::Health = resp.body_mut().read_json().ok()?;
    Some(health.version)
}

#[derive(Debug)]
pub(crate) enum CallError {
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
pub(crate) enum Retry {
    /// Fail fast (push/status/comment) — an immediate "is 'nit serve' running?"
    /// beats hanging.
    No,
    /// Keep retrying with backoff (`nit wait`/`--follow` riding out a restart).
    UntilUp,
}

fn retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(1 << attempt.min(4)).min(std::time::Duration::from_secs(10))
}

pub(crate) type WsConn =
    tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

pub(crate) struct Client {
    agent: ureq::Agent,
    base: String,
}

impl Client {
    pub(crate) fn new(base: String) -> Self {
        let config = ureq::config::Config::builder()
            .http_status_as_error(false)
            .build();
        Client {
            agent: config.new_agent(),
            base,
        }
    }

    pub(crate) fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
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

    pub(crate) fn get_retry<T: DeserializeOwned>(&self, path: &str, retry: Retry) -> Result<T> {
        self.retry_loop(retry, || self.get_raw(path))
    }

    /// `subs` maps `change_id` → from-idx.
    pub(crate) fn ws_connect(
        &self,
        subs: &std::collections::HashMap<u64, u64>,
        retry: Retry,
    ) -> Result<WsConn> {
        let url = format!("{}/api/stream", self.base.replacen("http", "ws", 1));
        let map: std::collections::HashMap<String, u64> =
            subs.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let sub = serde_json::to_string(&ClientMsg::Subscribe(map))?;
        self.retry_loop(retry, || Self::try_ws(&url, &sub))
    }

    fn try_ws(url: &str, sub: &str) -> Result<WsConn, CallError> {
        let (mut socket, _) = tungstenite::connect(url).map_err(|e| classify_ws(&e))?;
        socket
            .send(tungstenite::Message::Text(sub.to_string().into()))
            .map_err(|e| classify_ws(&e))?;
        Ok(socket)
    }

    pub(crate) fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.post_raw(path, body)
            .map_err(|e| e.into_error(&self.base))
    }

    pub(crate) fn patch<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.patch_raw(path, body)
            .map_err(|e| e.into_error(&self.base))
    }

    fn get_raw<T: DeserializeOwned>(&self, path: &str) -> Result<T, CallError> {
        let url = format!("{}{path}", self.base);
        let response = self.agent.get(&url).call().map_err(|e| classify(e, path))?;
        Self::read(response, path)
    }

    fn post_raw<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, CallError> {
        let url = format!("{}{path}", self.base);
        Self::send_raw(self.agent.post(&url), path, body)
    }

    fn patch_raw<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, CallError> {
        let url = format!("{}{path}", self.base);
        Self::send_raw(self.agent.patch(&url), path, body)
    }

    /// Send a JSON body on a built request (the verb-agnostic half of
    /// [`post_raw`]/[`patch_raw`]).
    fn send_raw<B: Serialize, T: DeserializeOwned>(
        req: ureq::RequestBuilder<ureq::typestate::WithBody>,
        path: &str,
        body: &B,
    ) -> Result<T, CallError> {
        let response = req.send_json(body).map_err(|e| classify(e, path))?;
        Self::read(response, path)
    }

    /// Deserialize a success body into `T`; on a non-2xx, decode the
    /// `{"error": …}` envelope into a fatal error.
    fn read<T: DeserializeOwned>(
        mut response: ureq::http::Response<ureq::Body>,
        path: &str,
    ) -> Result<T, CallError> {
        let status = response.status();
        if !status.is_success() {
            let message = response
                .body_mut()
                .read_json::<ApiError>()
                .map_or_else(|_| "unknown error".to_string(), |e| e.error);
            return Err(CallError::Fatal(anyhow!(
                "{path}: {} — {message}",
                status.as_u16()
            )));
        }
        response
            .body_mut()
            .read_json::<T>()
            .map_err(|e| classify(e, path))
    }
}

/// Pump the change-stream socket to its next text frame, answering pings
/// transparently. Returns the frame text, or `None` on a close/error so the
/// caller reconnects.
pub(crate) fn next_text(socket: &mut WsConn) -> Option<String> {
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(text)) => return Some(text.to_string()),
            Ok(tungstenite::Message::Ping(p)) => {
                let _ = socket.send(tungstenite::Message::Pong(p));
            }
            Ok(tungstenite::Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

pub(crate) fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
