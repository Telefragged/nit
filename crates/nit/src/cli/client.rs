//! HTTP/websocket transport for the CLI: the [`Client`] over the nit server's
//! JSON API, the unreachable-vs-fatal [`CallError`] split, the [`Retry`] policy
//! that rides out server restarts, and the shared `print_json`/`server_url`
//! helpers.

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

pub(crate) const DEFAULT_SERVER: &str = "http://127.0.0.1:8877";

pub(crate) fn server_url(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("NIT_SERVER").ok())
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
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

/// Backoff between reconnect attempts: 1, 2, 4, 8, then 10s, capped.
fn retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(1 << attempt.min(4)).min(std::time::Duration::from_secs(10))
}

/// A connected websocket to the nit server.
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

    pub(crate) fn get(&self, path: &str) -> Result<Value> {
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
    pub(crate) fn get_retry(&self, path: &str, retry: Retry) -> Result<Value> {
        self.retry_loop(retry, || self.get_raw(path))
    }

    /// Connect the change stream and `subscribe` `subs` (`change_id` →
    /// from-idx), retrying the connect while the server is unreachable.
    pub(crate) fn ws_connect(
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

    pub(crate) fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.post_raw(path, body)
            .map_err(|e| e.into_error(&self.base))
    }

    pub(crate) fn patch(&self, path: &str, body: &Value) -> Result<Value> {
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

pub(crate) fn print_json(value: &Value) -> Result<()> {
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
