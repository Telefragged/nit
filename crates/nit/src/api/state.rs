//! Shared server state: the in-memory fold of every chain's log, per-chain
//! locks + the live event broadcast, scan orchestration, the append-then-fold
//! primitive, and the API error type.
//!
//! Each chain's [`Projection`](crate::review::Projection) is rebuilt by
//! replaying its log on startup and kept current by [`commit_entries`],
//! which appends to the DB log and folds in lock-step under the chain lock,
//! then publishes each appended entry on the chain's broadcast channel for
//! live `/events` subscribers (docs/data-model.md "Concurrency").

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};
use std::time::{Duration, Instant};

use async_broadcast::{InactiveReceiver, Receiver, Sender};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, TransactionBehavior};
use serde_json::Value;
use tokio::sync::{Mutex, watch};

use crate::db;
use crate::enums::LogKind;
use crate::gitscan;
use crate::review::{self, Projection};

use super::types;

/// Scans younger than this are not repeated (reads serve the current fold).
const SCAN_THROTTLE: Duration = Duration::from_secs(2);

/// Per-chain live-event buffer. A subscriber that lags more than this many
/// appended entries behind is dropped with an overflow signal; `nit wait`
/// then reconnects at its cursor and re-reads the gap from the log. Chosen
/// far above the burst any single review action produces.
const EVENTS_BUFFER: usize = 256;

pub struct AppState {
    pub db_path: PathBuf,
    /// `http://<listen addr>` — prefix of every `web_url`.
    pub public_base: String,
    chains: StdMutex<HashMap<u64, Arc<ChainEntry>>>,
    /// Process-global allocator for fold-assigned ids (changes, comments,
    /// reviews). Initialized above every id in the log on startup so the
    /// flat `/api/{changes,comments}/{id}` endpoints stay unambiguous.
    next_id: AtomicU64,
    shutdown: watch::Sender<bool>,
}

/// Per-chain coordination: the serializing lock, the folded state, the
/// live-entry broadcast, and the scan throttle.
pub struct ChainEntry {
    /// Held by every appender (scan / review / reply / resolve / partial) so
    /// they serialize.
    pub gate: Mutex<()>,
    /// The fold; readers take `read()`, appenders `write()` (briefly, after
    /// the DB commit).
    pub proj: StdRwLock<Projection>,
    /// Live `/events` feed: each appended entry is published here for
    /// currently-connected subscribers.
    events: Sender<types::LogEntry>,
    /// A parked receiver kept for the chain's lifetime so the channel never
    /// closes for lack of subscribers (an empty `/events` audience is normal).
    events_keepalive: InactiveReceiver<types::LogEntry>,
    scan_at: StdMutex<Option<Instant>>,
}

impl ChainEntry {
    /// Read-lock the projection.
    ///
    /// # Panics
    /// When the projection lock is poisoned.
    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, Projection> {
        self.proj.read().expect("projection lock poisoned")
    }

    /// Publish a freshly appended entry to live `/events` subscribers.
    /// Best-effort: with no subscribers the channel is inactive and the
    /// entry is simply not delivered — it is durable in the log, so a later
    /// subscriber reads it from the backlog. An overflowing slow subscriber
    /// is signalled (and reconnects); it is never blocked on here.
    pub fn publish(&self, entry: types::LogEntry) {
        let _ = self.events.try_broadcast(entry);
    }

    /// An active subscription to this chain's live entry feed. Arm it
    /// **before** reading the log backlog so no append slips through the gap
    /// between the read and the subscription.
    pub fn subscribe(&self) -> Receiver<types::LogEntry> {
        self.events_keepalive.activate_cloned()
    }
}

impl AppState {
    /// Open the database, replay every chain's log into memory, and seed the
    /// id allocator above every id the logs already use.
    ///
    /// # Errors
    /// When the database can't be opened or a log fails to replay.
    pub fn load(db_path: PathBuf, public_base: String) -> anyhow::Result<Arc<Self>> {
        let conn = db::open(&db_path)?;
        let mut map = HashMap::new();
        let mut max_id = db::max_draft_id(&conn)?;
        for chain in db::all_chains(&conn)? {
            let rows = db::log_entries(&conn, chain.id, 0, None)?;
            max_id = max_id.max(review::max_assigned_id(&rows)?);
            let proj = review::replay(&chain, &rows)?;
            map.insert(chain.id, Arc::new(ChainEntry::new(proj)));
        }
        let (shutdown, _) = watch::channel(false);
        Ok(Arc::new(AppState {
            db_path,
            public_base,
            chains: StdMutex::new(map),
            next_id: AtomicU64::new(max_id + 1),
            shutdown,
        }))
    }

    /// Allocate the next fold-assigned id.
    pub fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Mark the server as shutting down, waking every subscribed event stream.
    pub fn begin_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Observe [`AppState::begin_shutdown`] (level-triggered).
    #[must_use]
    pub fn shutdown_watch(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    /// The coordination entry for a loaded chain, if any.
    ///
    /// # Panics
    /// When the chain map mutex is poisoned.
    pub fn chain_entry(&self, chain_id: u64) -> Option<Arc<ChainEntry>> {
        self.chains
            .lock()
            .expect("chain map poisoned")
            .get(&chain_id)
            .cloned()
    }

    /// Loaded chain ids, ascending.
    ///
    /// # Panics
    /// When the chain map mutex is poisoned.
    pub fn chain_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .chains
            .lock()
            .expect("chain map poisoned")
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Ensure a [`ChainEntry`] exists for `chain` (building it by replaying
    /// the chain's log), and return it. Refreshes `base` on an existing one.
    ///
    /// # Errors
    /// When replay fails.
    ///
    /// # Panics
    /// When the chain map mutex is poisoned.
    pub fn ensure_entry(
        &self,
        conn: &Connection,
        chain: &db::ChainRow,
    ) -> anyhow::Result<Arc<ChainEntry>> {
        if let Some(existing) = self.chain_entry(chain.id) {
            let mut proj = existing.proj.write().expect("projection lock poisoned");
            proj.base.clone_from(&chain.base);
            drop(proj);
            return Ok(existing);
        }
        let rows = db::log_entries(conn, chain.id, 0, None)?;
        self.next_id
            .fetch_max(review::max_assigned_id(&rows)? + 1, Ordering::SeqCst);
        let proj = review::replay(chain, &rows)?;
        let entry = Arc::new(ChainEntry::new(proj));
        let mut map = self.chains.lock().expect("chain map poisoned");
        Ok(map.entry(chain.id).or_insert(entry).clone())
    }

    /// Open a database connection (blocking — call inside `spawn_blocking`).
    ///
    /// # Errors
    /// See [`db::open`].
    pub fn open_db(&self) -> anyhow::Result<Connection> {
        db::open(&self.db_path)
    }
}

impl ChainEntry {
    fn new(proj: Projection) -> ChainEntry {
        let (mut events, rx) = async_broadcast::broadcast(EVENTS_BUFFER);
        // Overflow rather than block: a publisher (holding the chain gate)
        // must never stall on a slow subscriber. An overflowed subscriber
        // gets an explicit signal and reconnects.
        events.set_overflow(true);
        ChainEntry {
            gate: Mutex::new(()),
            proj: StdRwLock::new(proj),
            events,
            events_keepalive: rx.deactivate(),
            scan_at: StdMutex::new(None),
        }
    }
}

/// Persist a batch of entries (one transaction), fold them into the
/// projection, and publish each on the chain's live `/events` feed; returns
/// whether anything was appended. The caller holds the chain gate. Entries
/// are appended at the projection's current head onward.
///
/// The fold is validated on a throwaway copy of the projection **before**
/// the database commit: a payload that won't fold errors out with nothing
/// written, so the log can never get ahead of the projection (which would
/// wedge replay on restart). Persisting before the live fold is deliberate
/// the other way too — a busy-DB abort then leaves the live projection
/// untouched.
///
/// # Errors
/// On a database failure (the projection is left untouched) or a fold
/// failure (nothing is written).
///
/// # Panics
/// When the projection lock is poisoned.
pub fn commit_entries(
    conn: &mut Connection,
    entry: &ChainEntry,
    chain_id: u64,
    news: Vec<(LogKind, Value)>,
) -> anyhow::Result<bool> {
    if news.is_empty() {
        return Ok(false);
    }
    let now = db::now_rfc3339();
    let start = entry.read().head;
    let parsed: Vec<review::Entry> = news
        .into_iter()
        .enumerate()
        .map(|(k, (kind, payload))| review::Entry {
            idx: start + u64::try_from(k).expect("batch fits u64"),
            kind,
            payload,
            created_at: now.clone(),
        })
        .collect();

    // Validate the fold on a probe copy; a failure here aborts before any
    // write. The live fold below is then infallible.
    let mut probe = entry.read().clone();
    for e in &parsed {
        review::fold(&mut probe, e)?;
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for e in &parsed {
        db::append_log(&tx, chain_id, e.idx, e.kind.as_str(), &e.payload, &now)?;
    }
    tx.commit()?;

    {
        let mut proj = entry.proj.write().expect("projection lock poisoned");
        for e in &parsed {
            review::fold(&mut proj, e).expect("fold validated before commit");
        }
    }
    // Publish only after the durable commit + fold, so a subscriber that
    // also reads this entry from its backlog sees a consistent log.
    for e in parsed {
        entry.publish(super::views::log_entry_view(&e));
    }
    Ok(true)
}

/// Run a scan for `chain_id` and apply its result under the chain lock.
/// `force` skips the throttle (`nit push` always rescans; dashboard reads
/// don't).
///
/// # Errors
/// Only infrastructure problems (broken db) — scan-level git failures land
/// in the projection's `last_scan_error` instead.
///
/// # Panics
/// When the projection or scan-throttle lock is poisoned.
pub async fn scan_chain(state: &Arc<AppState>, chain_id: u64, force: bool) -> Result<(), Error> {
    let Some(entry) = state.chain_entry(chain_id) else {
        return Ok(());
    };
    let guard = if force {
        entry.gate.lock().await
    } else {
        match entry.gate.try_lock() {
            Ok(guard) => guard,
            Err(_) => return Ok(()), // reads never wait on a running scan
        }
    };
    if !force {
        let last = *entry.scan_at.lock().expect("scan throttle poisoned");
        if last.is_some_and(|at| at.elapsed() < SCAN_THROTTLE) {
            return Ok(());
        }
    }

    let st = state.clone();
    let e2 = entry.clone();
    blocking(move || -> Result<(), Error> {
        let mut conn = st.open_db()?;
        let snapshot = e2.read().clone();
        let mut alloc = || st.alloc_id();
        let result = gitscan::scan(&snapshot, jiff::Timestamp::now(), &mut alloc);
        let error = result.error;
        let branch_missing_since = result.branch_missing_since;
        let news: Vec<(LogKind, Value)> = result
            .entries
            .into_iter()
            .map(|n| (n.kind, n.payload))
            .collect();
        // Commit first; apply the scan's transient state only when the
        // entries actually landed. A busy-dropped abandon would otherwise
        // null the branch-missing timer without closing the chain, so the
        // 10s window could never accumulate under sustained contention.
        match commit_entries(&mut conn, &e2, chain_id, news) {
            Ok(_) => {}
            Err(err) if is_sqlite_busy(&err) => {
                return if force {
                    Err(Error::unavailable(
                        "database is busy (another chain is being scanned) — retry shortly",
                    ))
                } else {
                    Ok(()) // transient state untouched: the prior timer survives
                };
            }
            Err(err) => return Err(err.into()),
        }
        let mut proj = e2.proj.write().expect("projection lock poisoned");
        proj.last_scan_error = error;
        proj.branch_missing_since = branch_missing_since;
        Ok(())
    })
    .await?;

    *entry.scan_at.lock().expect("scan throttle poisoned") = Some(Instant::now());
    drop(guard);
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
/// bodies) into the documented `{"error": …}` JSON shape.
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
            // A body that won't deserialize is bad input — including an
            // unknown enum value (a malformed `verdict`/`side`/…). axum
            // reports a data error as 422, but nit speaks 400 for every bad
            // request body (see `Error::bad_request`), so normalize it.
            Err(rej) => Err(Error {
                status: StatusCode::BAD_REQUEST,
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
