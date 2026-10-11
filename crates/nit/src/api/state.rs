//! Shared server state.
//!
//! The repo registry cache, the projection cache, the append primitive, the
//! per-repo appended watch, and the API error type.
//!
//! The log is the only record of a change. A read replays it into a
//! [`ChangeProjection`]. An append replays it inside the write transaction
//! that extends it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};

use async_broadcast::{InactiveReceiver, Receiver, Sender};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use deadpool_sqlite::Pool;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use tokio::sync::watch;

use nit_types::changes::ChangeQuery;
use nit_types::domain::ChangeNumber;
use nit_types::domain::{LogEntry, LogPayload};
use nit_types::error::ApiError;

use crate::db;
use nit_types::domain::ChangeProjection;

use crate::review;

/// Live-event buffer.
///
/// The channel carries every change's entries, so this covers the server's
/// traffic rather than one change's burst. A follower lagging more than this
/// many entries behind is dropped from the stream and reconnects + re-reads
/// the gap from the log (the log is the source of truth).
const EVENTS_BUFFER: usize = 1024;

pub struct AppState {
    pool: Pool,
    repos: StdMutex<HashMap<u64, Arc<RepoState>>>,
    /// Every appended entry, for every change.
    ///
    /// One channel carries the entries of every change, whatever its
    /// lifecycle. A follower filters on the repo and tags sent with each
    /// entry.
    events: Sender<Published>,
    /// A parked receiver so the channel never closes for lack of followers.
    events_keepalive: InactiveReceiver<Published>,
    /// Process-global allocator for draft ids.
    ///
    /// Seeded past the `draft_comments` rows in use. Change numbers are
    /// `changes` rowids and review ids are log coordinates — neither is
    /// allocated here.
    next_id: AtomicU64,
    shutdown: watch::Sender<bool>,
}

/// Cached repo registry entry.
///
/// `git_dir` is in an `RwLock` because `nit repo move` repoints it in-place.
/// The merge timer's baseline is not cached here — it lives only in
/// `repos.canonical_head` (the merge sweep's cursor, `crate::api::timer`).
pub struct RepoState {
    pub canonical_ref: String,
    pub git_dir: StdRwLock<String>,
}

impl RepoState {
    fn new(row: &db::RepoRow) -> RepoState {
        RepoState {
            canonical_ref: row.canonical_ref.clone(),
            git_dir: StdRwLock::new(row.git_dir.clone()),
        }
    }

    /// The current git-common-dir.
    ///
    /// # Panics
    ///
    /// When the git-dir lock is poisoned.
    #[must_use]
    pub fn git_dir(&self) -> String {
        self.git_dir.read().expect("git_dir lock poisoned").clone()
    }
}

/// One appended entry, as the event channel sends it to followers.
///
/// `change` is the projection after the last entry of its append was
/// folded. It
/// travels with the entry so a follower can filter and announce without
/// replaying the change.
#[derive(Debug, Clone)]
pub struct Published {
    pub entry: LogEntry,
    pub change: Arc<ChangeProjection>,
}

impl AppState {
    /// Initializes the state from `db_path`.
    ///
    /// Migrates the schema and caches the repo registry.
    /// Seeds the id allocator above every draft id in use to prevent reuse
    /// after restart.
    ///
    /// # Errors
    ///
    /// When the pool fails to build, or the schema migration fails.
    pub async fn load(db_path: PathBuf) -> anyhow::Result<Arc<Self>> {
        let pool = db::pool(&db_path)?;
        let conn = pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("database pool: {e}"))?;
        let (repos, max_id) = conn
            .interact(|conn| -> anyhow::Result<_> {
                db::migrate(conn)?;
                let max_id = db::max_draft_id(conn)?;
                let repos: HashMap<u64, Arc<RepoState>> = db::all_repos(conn)?
                    .into_iter()
                    .map(|r| (r.id, Arc::new(RepoState::new(&r))))
                    .collect();
                Ok((repos, max_id))
            })
            .await
            .map_err(|e| anyhow::anyhow!("database init: {e}"))??;
        let (shutdown, _) = watch::channel(false);
        let (mut events, rx) = async_broadcast::broadcast(EVENTS_BUFFER);
        // A publisher (holding no async lock) must never stall on a slow
        // follower; an overflowed follower reconnects and re-reads the gap from
        // the log.
        events.set_overflow(true);
        Ok(Arc::new(AppState {
            pool,
            repos: StdMutex::new(repos),
            events,
            events_keepalive: rx.deactivate(),
            next_id: AtomicU64::new(max_id + 1),
            shutdown,
        }))
    }

    /// Publishes an entry to live followers.
    ///
    /// Best-effort: with none, the channel is inactive and the message is
    /// dropped (it is durable in the log).
    pub fn publish(&self, msg: Published) {
        let _ = self.events.try_broadcast(msg);
    }

    /// A subscription to the server's event stream, carrying every change.
    ///
    /// It sees only entries published after this call, so arming it
    /// **before** reading a backlog leaves no gap for an append to slip
    /// through.
    pub fn subscribe(&self) -> Receiver<Published> {
        self.events_keepalive.activate_cloned()
    }

    pub fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Signals shutdown, waking every task subscribed via [`Self::shutdown_watch`].
    pub fn begin_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Observe [`AppState::begin_shutdown`] (level-triggered).
    #[must_use]
    pub fn shutdown_watch(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    /// The cached repo state, if loaded.
    ///
    /// # Panics
    ///
    /// When the repo map mutex is poisoned.
    pub fn repo_state(&self, repo_id: u64) -> Option<Arc<RepoState>> {
        self.repos
            .lock()
            .expect("repo map poisoned")
            .get(&repo_id)
            .cloned()
    }

    /// The git-common-dir of a loaded repo (the read-side git work needs it).
    ///
    /// # Errors
    ///
    /// When the repo is not in the registry cache.
    pub fn git_dir(&self, repo_id: u64) -> Result<String, Error> {
        Ok(self
            .repo_state(repo_id)
            .ok_or_else(|| Error::internal("repo not loaded"))?
            .git_dir())
    }

    /// Loaded repo ids, ascending.
    ///
    /// # Panics
    ///
    /// When the repo map mutex is poisoned.
    pub fn repo_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .repos
            .lock()
            .expect("repo map poisoned")
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Cache (or refresh the git dir of) a repo's registry row.
    ///
    /// # Panics
    ///
    /// When the repo map mutex is poisoned.
    pub fn ensure_repo(&self, row: &db::RepoRow) -> Arc<RepoState> {
        let mut map = self.repos.lock().expect("repo map poisoned");
        if let Some(existing) = map.get(&row.id) {
            existing
                .git_dir
                .write()
                .expect("git_dir lock poisoned")
                .clone_from(&row.git_dir);
            return existing.clone();
        }
        let state = Arc::new(RepoState::new(row));
        map.insert(row.id, state.clone());
        state
    }

    /// Returns the folds of the changes a query picks, as owned
    /// projections, ascending by change number within each repo.
    ///
    /// # Errors
    ///
    /// When enumerating or replaying a change fails.
    pub fn changes_matching(
        &self,
        conn: &Connection,
        query: ChangeQuery,
    ) -> anyhow::Result<Vec<ChangeProjection>> {
        let repo_ids = self.repo_ids_matching(query.repo);
        let filter = db::ChangeFilter::from(query);
        let mut changes = Vec::new();
        for repo_id in repo_ids {
            changes.extend(review::repo_changes(conn, repo_id, &filter)?);
        }
        Ok(changes)
    }

    /// Returns the repo ids a `?repo=` filter admits.
    ///
    /// An unknown id admits none, so a list read filters to empty rather
    /// than 404ing.
    #[must_use]
    pub fn repo_ids_matching(&self, repo: Option<u64>) -> Vec<u64> {
        self.repo_ids()
            .into_iter()
            .filter(|&id| repo.is_none_or(|r| r == id))
            .collect()
    }

    /// A handle to the connection pool (cheaply cloned — it is `Arc`-backed).
    #[must_use]
    pub fn pool(&self) -> Pool {
        self.pool.clone()
    }
}

/// The projection of a change after an append, and the entries that the
/// append added.
pub struct Appended {
    pub change: ChangeProjection,
    pub entries: Vec<LogEntry>,
}

/// A write transaction that appends to the logs of one or more changes.
///
/// It opens with `BEGIN IMMEDIATE`, so it holds the database write lock
/// before its first read. Each append replays the change and appends at
/// its head. No other writer can move that head before the commit. The
/// entries reach followers only after [`Appends::commit`].
pub struct Appends<'c> {
    tx: Transaction<'c>,
    published: Vec<Published>,
}

impl<'c> Appends<'c> {
    /// Opens the write transaction.
    ///
    /// # Errors
    ///
    /// When another writer holds the database past the busy timeout.
    pub fn begin(conn: &'c mut Connection) -> Result<Appends<'c>, Error> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Ok(Appends {
            tx,
            published: Vec::new(),
        })
    }

    /// Appends to a change the payloads that `new_payloads` returns for the
    /// current projection of the change.
    ///
    /// `new_payloads` reads the change inside this transaction, so its
    /// result stays valid until the commit. The fold allocates the ids of
    /// new threads. The write lock makes that allocation race-free.
    ///
    /// # Errors
    ///
    /// 404 for a change number that is absent from the `changes` table, the
    /// error that `new_payloads` returns, or a database failure. The caller
    /// then drops the transaction. The drop rolls back every write in it.
    pub fn append(
        &mut self,
        change_number: ChangeNumber,
        new_payloads: impl FnOnce(&ChangeProjection, &Transaction) -> Result<Vec<LogPayload>, Error>,
    ) -> Result<Appended, Error> {
        let mut change = super::change_or_404(&self.tx, change_number)?;
        let news = new_payloads(&change, &self.tx)?;
        if news.is_empty() {
            return Ok(Appended {
                change,
                entries: Vec::new(),
            });
        }
        let retags = news.iter().any(|p| matches!(p, LogPayload::Tags(_)));
        let now = db::now_rfc3339();
        let start = change.entries_folded;
        let mut entries = Vec::with_capacity(news.len());
        for (position, payload) in (start..).zip(news) {
            let entry = review::fold(
                &mut change,
                LogEntry {
                    change_number,
                    sequence: 0,
                    position,
                    created_at: now.clone(),
                    payload,
                },
            );
            let payload = review::payload_to_json(&entry.payload)?;
            let sequence = db::append_log(
                &self.tx,
                change_number,
                entry.position,
                entry.payload.kind().as_str(),
                &payload,
                &now,
            )?;
            entries.push(LogEntry { sequence, ..entry });
        }
        // These writes update the denormalized columns that the change filter
        // reads.
        db::update_change_status(&self.tx, change_number, change.current_status())?;
        if retags {
            db::set_change_tags(&self.tx, change_number, &change.tags)?;
        }
        let snapshot = Arc::new(change.clone());
        self.published.extend(entries.iter().map(|entry| Published {
            entry: entry.clone(),
            change: Arc::clone(&snapshot),
        }));
        Ok(Appended { change, entries })
    }

    /// Commits the transaction, then publishes its entries to followers.
    ///
    /// # Errors
    ///
    /// When the commit fails. Nothing is published then.
    pub fn commit(self, state: &AppState) -> Result<(), Error> {
        self.tx.commit()?;
        for msg in self.published {
            state.publish(msg);
        }
        Ok(())
    }
}

/// Appends to a change in a separate transaction.
///
/// See [`Appends::append`].
///
/// # Errors
///
/// See [`Appends::append`] and [`Appends::commit`].
pub fn append(
    state: &AppState,
    conn: &mut Connection,
    change_number: ChangeNumber,
    new_payloads: impl FnOnce(&ChangeProjection, &Transaction) -> Result<Vec<LogPayload>, Error>,
) -> Result<Appended, Error> {
    let mut appends = Appends::begin(conn)?;
    let appended = appends.append(change_number, new_payloads)?;
    appends.commit(state)?;
    Ok(appended)
}

/// Runs blocking rusqlite/git2 work on a pooled connection.
///
/// Acquires the connection and runs the work off the async runtime —
/// deadpool's `interact` hands the closure a `&mut Connection` on a blocking
/// thread. The per-request connection accessor every handler routes its work
/// through.
///
/// # Errors
///
/// When the pool can't hand out a connection, the blocking task is cancelled or
/// panics, or `f` itself returns an error.
pub async fn with_conn<T, F>(pool: Pool, f: F) -> Result<T, Error>
where
    T: Send + 'static,
    F: FnOnce(&mut Connection) -> Result<T, Error> + Send + 'static,
{
    let conn = pool
        .get()
        .await
        .map_err(|e| Error::internal(format!("database pool: {e}")))?;
    conn.interact(f)
        .await
        .map_err(|e| Error::internal(format!("database task: {e}")))?
}

// Non-2xx responses carry {"error": "human readable message"} in the body.

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

/// Extractor wrappers over axum's built-in rejections.
///
/// They turn its text/plain bodies into the documented `{"error": …}` JSON
/// shape.
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
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // serde_html_form, not axum's serde_urlencoded-backed Query: a
        // repeated key (`?status=a&status=b`) deserializes into a Vec.
        serde_html_form::from_str(parts.uri.query().unwrap_or_default())
            .map(AppQuery)
            .map_err(|e| Error::bad_request(format!("invalid query string: {e}")))
    }
}

/// `SQLITE_BUSY/LOCKED` anywhere in an error chain.
///
/// The error means write contention, and the database itself is healthy.
#[must_use]
fn is_sqlite_busy(err: &anyhow::Error) -> bool {
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
        if is_sqlite_busy(&err) {
            Error::unavailable(
                "database is busy because another write is in progress. Retry the request",
            )
        } else {
            Error::internal(format!("{err:#}"))
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        anyhow::Error::from(err).context("database error").into()
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!("{}: {}", self.status, self.message);
        }
        (
            self.status,
            Json(ApiError {
                error: self.message,
            }),
        )
            .into_response()
    }
}
