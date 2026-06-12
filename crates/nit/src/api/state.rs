//! Shared server state: per-chain locks + notifications, scan
//! orchestration (throttle, error isolation) and the API error type.
//!
//! Concurrency contract (docs/data-model.md "Concurrency"): one per-chain
//! async mutex serializes every scan of a chain *and* every review
//! submission to it; scans < 2s old are not repeated; a failing chain
//! never breaks the others.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::Connection;
use tokio::sync::{Mutex, Notify};

use crate::db;
use crate::gitscan;

use super::types;

/// Scans younger than this are not repeated (reads serve current DB state).
const SCAN_THROTTLE: Duration = Duration::from_secs(2);

pub struct AppState {
    pub db_path: PathBuf,
    /// `http://<listen addr>` — prefix of every `web_url`.
    pub public_base: String,
    chains: StdMutex<HashMap<i64, Arc<ChainEntry>>>,
}

/// Per-chain coordination: the serializing lock, the `/wait` wakeup
/// channel, and the latest scan's warnings (per-scan data, not persisted —
/// docs/api.md `scan_warnings`).
pub struct ChainEntry {
    pub gate: Mutex<ScanGate>,
    pub notify: Notify,
    warnings: StdMutex<Vec<String>>,
}

#[derive(Default)]
pub struct ScanGate {
    last_scan: Option<Instant>,
}

impl AppState {
    #[must_use]
    pub fn new(db_path: PathBuf, public_base: String) -> Arc<Self> {
        Arc::new(AppState {
            db_path,
            public_base,
            chains: StdMutex::new(HashMap::new()),
        })
    }

    /// The coordination entry for a chain (created on first touch).
    ///
    /// # Panics
    /// When the chain-map mutex is poisoned.
    pub fn entry(&self, chain_id: i64) -> Arc<ChainEntry> {
        let mut map = self.chains.lock().expect("chain map poisoned");
        map.entry(chain_id)
            .or_insert_with(|| {
                Arc::new(ChainEntry {
                    gate: Mutex::new(ScanGate::default()),
                    notify: Notify::new(),
                    warnings: StdMutex::new(Vec::new()),
                })
            })
            .clone()
    }

    /// Open a database connection (blocking — call inside
    /// `spawn_blocking`).
    ///
    /// # Errors
    /// See [`db::open`].
    pub fn open_db(&self) -> anyhow::Result<Connection> {
        db::open(&self.db_path)
    }

    /// The latest scan's warnings for a chain (empty until a scan ran in
    /// this server's lifetime).
    ///
    /// # Panics
    /// When the warnings mutex is poisoned.
    pub fn scan_warnings(&self, chain_id: i64) -> Vec<String> {
        self.entry(chain_id)
            .warnings
            .lock()
            .expect("warnings poisoned")
            .clone()
    }
}

/// Run a scan for `chain_id` under its chain lock. `force` skips the
/// throttle (`nit push` always rescans; dashboard reads don't).
///
/// # Errors
/// Only infrastructure problems (broken db) — scan-level git failures
/// land in `last_scan_error` instead (error isolation).
///
/// # Panics
/// When the warnings mutex is poisoned.
pub async fn scan_chain(state: &Arc<AppState>, chain_id: i64, force: bool) -> Result<(), Error> {
    let entry = state.entry(chain_id);
    let mut gate = if force {
        entry.gate.lock().await
    } else {
        // Reads never wait on a running scan — they serve current DB state
        // (data-model.md "Concurrency").
        match entry.gate.try_lock() {
            Ok(gate) => gate,
            Err(_) => return Ok(()),
        }
    };
    if !force
        && gate
            .last_scan
            .is_some_and(|at| at.elapsed() < SCAN_THROTTLE)
    {
        return Ok(());
    }
    let st = state.clone();
    let outcome = blocking(move || {
        let mut conn = st.open_db()?;
        match gitscan::scan(&mut conn, chain_id) {
            Ok(outcome) => Ok(Some(outcome)),
            // Another chain's scan holds the db write lock past the busy
            // timeout: skip — previous state stays served, the next read
            // retries. Pushes (force) report it: the caller must retry.
            Err(err) if is_sqlite_busy(&err) => {
                if force {
                    Err(Error::unavailable(
                        "database is busy (another chain is being scanned) — retry shortly",
                    ))
                } else {
                    Ok(None)
                }
            }
            Err(err) => Err(err.into()),
        }
    })
    .await?;
    let Some(outcome) = outcome else {
        return Ok(());
    };
    gate.last_scan = Some(Instant::now());
    *entry.warnings.lock().expect("warnings poisoned") = outcome.warnings;
    drop(gate);
    if outcome.updated {
        entry.notify.notify_waiters();
    }
    Ok(())
}

/// Run blocking (rusqlite / git2) work off the async threads.
///
/// # Errors
/// Whatever `f` returns; a panicked task maps to a 500.
pub async fn blocking<T, F>(f: F) -> Result<T, Error>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Error> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::internal(format!("blocking task panicked: {e}")))?
}

// ---------------------------------------------------------------------------
// Error type: non-2xx with {"error": "human readable message"}

#[derive(Debug)]
pub struct Error {
    pub status: StatusCode,
    pub message: String,
}

impl Error {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Error {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Error {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Error {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Error {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Error {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

/// Extractor wrappers that turn axum's built-in rejections (text/plain
/// bodies) into the documented `{"error": …}` JSON shape — api.md promises
/// it for *every* non-2xx under /api.
pub struct AppJson<T>(pub T);

impl<S, T> axum::extract::FromRequest<S> for AppJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(AppJson(value)),
            Err(rej) => Err(Error {
                status: rej.status(),
                message: rej.body_text(),
            }),
        }
    }
}

pub struct AppPath<T>(pub T);

impl<S, T> axum::extract::FromRequestParts<S> for AppPath<T>
where
    T: serde::de::DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(AppPath(value)),
            Err(rej) => Err(Error {
                status: rej.status(),
                message: rej.body_text(),
            }),
        }
    }
}

pub struct AppQuery<T>(pub T);

impl<S, T> axum::extract::FromRequestParts<S> for AppQuery<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(AppQuery(value)),
            Err(rej) => Err(Error {
                status: rej.status(),
                message: rej.body_text(),
            }),
        }
    }
}

/// `SQLITE_BUSY/LOCKED` anywhere in an error chain: cross-chain write
/// contention, not a broken database.
#[must_use]
pub fn is_sqlite_busy(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause.downcast_ref::<rusqlite::Error>().is_some_and(|e| {
            matches!(
                e.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
            )
        })
    })
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::internal(format!("{err:#}"))
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::internal(format!("database error: {err}"))
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!("{}: {}", self.status, self.message);
        }
        (
            self.status,
            Json(types::ApiError {
                error: self.message,
            }),
        )
            .into_response()
    }
}
