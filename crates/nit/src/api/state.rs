//! Shared server state: the in-memory fold of every **change's** log, the
//! repo registry cache, the per-change append primitive, and the API error
//! type.
//!
//! Each change's [`ChangeProj`](crate::review::ChangeProj) is rebuilt by
//! replaying its log on startup and kept current by [`append_to_change`],
//! which appends to the DB log and folds in lock-step under the change's
//! projection write lock (docs/data-model.md "Concurrency"). A chain owns no
//! state — it is derived at read time from member folds (`crate::chain`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};

use async_broadcast::{InactiveReceiver, Receiver, Sender};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, TransactionBehavior};
use serde_json::Value;
use tokio::sync::watch;

use crate::chain::RepoView;
use crate::db;
use crate::enums::LogKind;
use crate::review::{self, ChangeProj};

use super::types::{self, StreamMsg};

/// Per-change live-event buffer. A follower lagging more than this many entries
/// behind is dropped from the stream and reconnects + re-reads the gap from the
/// log (the log is the source of truth). Far above any single review burst.
const EVENTS_BUFFER: usize = 256;

pub struct AppState {
    pub db_path: PathBuf,
    /// `http://<listen addr>` — prefix of every `web_url`.
    pub public_base: String,
    repos: StdMutex<HashMap<u64, Arc<RepoState>>>,
    changes: StdMutex<HashMap<u64, Arc<ChangeEntry>>>,
    /// Process-global allocator for fold-assigned ids (reviews, drafts). Change
    /// ids are `changes` rowids, never allocated here.
    next_id: AtomicU64,
    shutdown: watch::Sender<bool>,
}

/// Cached repo registry row: identity, the one canonical branch, and the git
/// dir (mutable on `nit repo move`).
pub struct RepoState {
    pub id: u64,
    pub base_branch: String,
    pub git_dir: StdRwLock<String>,
}

impl RepoState {
    fn new(row: &db::RepoRow) -> RepoState {
        RepoState {
            id: row.id,
            base_branch: row.base_branch.clone(),
            git_dir: StdRwLock::new(row.git_dir.clone()),
        }
    }

    /// The current git-common-dir.
    ///
    /// # Panics
    /// When the git-dir lock is poisoned.
    #[must_use]
    pub fn git_dir(&self) -> String {
        self.git_dir.read().expect("git_dir lock poisoned").clone()
    }
}

/// Per-change coordination: the folded state behind one `RwLock` (appenders
/// take it for `write`, which both serializes them and guards the fold;
/// readers take it for `read`; held only inside `spawn_blocking`, never across
/// `.await`) plus the live-event broadcast for websocket followers.
pub struct ChangeEntry {
    pub proj: StdRwLock<ChangeProj>,
    /// Live feed: each appended entry is published here for current followers.
    events: Sender<StreamMsg>,
    /// A parked receiver so the channel never closes for lack of followers.
    events_keepalive: InactiveReceiver<StreamMsg>,
}

impl ChangeEntry {
    fn new(proj: ChangeProj) -> ChangeEntry {
        let (mut events, rx) = async_broadcast::broadcast(EVENTS_BUFFER);
        // Overflow rather than block: a publisher (holding no async lock) must
        // never stall on a slow follower. An overflowed follower reconnects and
        // re-reads the gap from the log.
        events.set_overflow(true);
        ChangeEntry {
            proj: StdRwLock::new(proj),
            events,
            events_keepalive: rx.deactivate(),
        }
    }

    /// Read-lock the projection.
    ///
    /// # Panics
    /// When the projection lock is poisoned.
    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, ChangeProj> {
        self.proj.read().expect("projection lock poisoned")
    }

    /// Publish a message to live followers. Best-effort: with none, the channel
    /// is inactive and the message is dropped (it is durable in the log).
    pub fn publish(&self, msg: StreamMsg) {
        let _ = self.events.try_broadcast(msg);
    }

    /// An active subscription to this change's live feed. Arm it **before**
    /// reading the backlog so no append slips the arm/read gap.
    pub fn subscribe(&self) -> Receiver<StreamMsg> {
        self.events_keepalive.activate_cloned()
    }
}

impl AppState {
    /// Open the database, replay every change's log into memory, cache the repo
    /// registry, and seed the id allocator above every fold id in use.
    ///
    /// # Errors
    /// When the database can't be opened or a log fails to replay.
    pub fn load(db_path: PathBuf, public_base: String) -> anyhow::Result<Arc<Self>> {
        let conn = db::open(&db_path)?;
        let mut max_id = db::max_draft_id(&conn)?;
        let mut changes = HashMap::new();
        for row in db::all_changes(&conn)? {
            let rows = db::log_entries(&conn, row.id, 0, None)?;
            max_id = max_id.max(review::max_assigned_id(&rows)?);
            let proj = review::replay(&row, &rows)?;
            changes.insert(row.id, Arc::new(ChangeEntry::new(proj)));
        }
        let repos = db::all_repos(&conn)?
            .into_iter()
            .map(|r| (r.id, Arc::new(RepoState::new(&r))))
            .collect();
        let (shutdown, _) = watch::channel(false);
        Ok(Arc::new(AppState {
            db_path,
            public_base,
            repos: StdMutex::new(repos),
            changes: StdMutex::new(changes),
            next_id: AtomicU64::new(max_id + 1),
            shutdown,
        }))
    }

    /// Allocate the next fold-assigned id (reviews, drafts).
    pub fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Mark the server as shutting down, waking every parked task.
    pub fn begin_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Observe [`AppState::begin_shutdown`] (level-triggered).
    #[must_use]
    pub fn shutdown_watch(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    /// The coordination entry for a loaded change, if any.
    ///
    /// # Panics
    /// When the change map mutex is poisoned.
    pub fn change_entry(&self, change_id: u64) -> Option<Arc<ChangeEntry>> {
        self.changes
            .lock()
            .expect("change map poisoned")
            .get(&change_id)
            .cloned()
    }

    /// Loaded change ids, ascending.
    ///
    /// # Panics
    /// When the change map mutex is poisoned.
    pub fn change_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .changes
            .lock()
            .expect("change map poisoned")
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        ids
    }

    /// The cached repo state, if loaded.
    ///
    /// # Panics
    /// When the repo map mutex is poisoned.
    pub fn repo_state(&self, repo_id: u64) -> Option<Arc<RepoState>> {
        self.repos
            .lock()
            .expect("repo map poisoned")
            .get(&repo_id)
            .cloned()
    }

    /// Loaded repo ids, ascending.
    ///
    /// # Panics
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

    /// Ensure a [`ChangeEntry`] exists for a change row (an empty projection if
    /// new), and return it. Replays the change's log if it is already on disk.
    ///
    /// # Errors
    /// When replay fails.
    ///
    /// # Panics
    /// When the change map mutex is poisoned.
    pub fn ensure_change(
        &self,
        conn: &Connection,
        row: &db::ChangeRow,
    ) -> anyhow::Result<Arc<ChangeEntry>> {
        if let Some(existing) = self.change_entry(row.id) {
            return Ok(existing);
        }
        let rows = db::log_entries(conn, row.id, 0, None)?;
        self.next_id
            .fetch_max(review::max_assigned_id(&rows)? + 1, Ordering::SeqCst);
        let proj = review::replay(row, &rows)?;
        let entry = Arc::new(ChangeEntry::new(proj));
        let mut map = self.changes.lock().expect("change map poisoned");
        Ok(map.entry(row.id).or_insert(entry).clone())
    }

    /// Snapshot every loaded change of one repo (each cloned out from under its
    /// lock), and build a [`RepoView`] for chain derivation.
    ///
    /// # Panics
    /// When the change map mutex is poisoned.
    #[must_use]
    pub fn repo_view(&self, repo_id: u64) -> RepoView {
        let entries: Vec<Arc<ChangeEntry>> = {
            let map = self.changes.lock().expect("change map poisoned");
            map.values().cloned().collect()
        };
        let changes: Vec<ChangeProj> = entries
            .iter()
            .map(|e| e.read().clone())
            .filter(|c| c.repo_id == repo_id)
            .collect();
        RepoView::new(changes)
    }

    /// Open a database connection (blocking — call inside `spawn_blocking`).
    ///
    /// # Errors
    /// See [`db::open`].
    pub fn open_db(&self) -> anyhow::Result<Connection> {
        db::open(&self.db_path)
    }
}

/// Append a batch of entries to one change (one transaction), folding them in
/// lock-step, and return the applied entries (with their minted `seq`). See
/// [`append_to_change_with`]; this is the no-extra-work case.
///
/// # Errors
/// See [`append_to_change_with`].
pub fn append_to_change(
    conn: &mut Connection,
    entry: &ChangeEntry,
    change_id: u64,
    news: Vec<(LogKind, Value)>,
) -> anyhow::Result<Vec<review::Entry>> {
    append_to_change_with(conn, entry, change_id, news, |_| Ok(()))
}

/// Append entries to one change, running `pre_commit` inside the **same**
/// transaction first (e.g. draining drafts atomically with a `review` append).
/// The change's projection write lock serializes appenders, so the
/// committed-state `idx` is consistent and applies happen in order — no
/// reorder buffer needed. The lock spans the commit, so a reader can briefly
/// stall behind an in-flight append; cross-change appends never contend.
///
/// The new entries are folded into a **clone** of the projection **before** the
/// commit; a payload that won't fold errors out with nothing written, so the
/// log can never get ahead of the projection. The clone that validated is then
/// installed verbatim after the commit (no second fold). `pre_commit` and the
/// appends share one transaction, so either both land or neither does.
///
/// # Errors
/// On a database failure (the projection is left untouched), a fold failure
/// (nothing is written), or a `pre_commit` failure (the transaction rolls
/// back).
///
/// # Panics
/// When the projection lock is poisoned.
pub fn append_to_change_with(
    conn: &mut Connection,
    entry: &ChangeEntry,
    change_id: u64,
    news: Vec<(LogKind, Value)>,
    pre_commit: impl FnOnce(&rusqlite::Transaction) -> anyhow::Result<()>,
) -> anyhow::Result<Vec<review::Entry>> {
    if news.is_empty() {
        return Ok(Vec::new());
    }
    // The write lock both serializes appenders and guards the fold; held
    // across the commit so the log can never get ahead of the projection.
    let mut proj = entry.proj.write().expect("projection lock poisoned");
    let now = db::now_rfc3339();

    // Build the next projection on a clone and fold the new entries into it: a
    // bad payload aborts here, before any write. The validated clone is then
    // installed verbatim after the commit — one fold, not two, and the
    // installed object is provably the one that validated (the fold ignores the
    // global `seq`, so the clone equals what re-folding the committed rows
    // gives).
    let start = db::log_head(conn, change_id)?;
    let mut next = proj.clone();
    let staged: Vec<review::Entry> = news
        .into_iter()
        .enumerate()
        .map(|(k, (kind, payload))| review::Entry {
            seq: 0,
            idx: start + u64::try_from(k).expect("batch fits u64"),
            kind,
            payload,
            created_at: now.clone(),
        })
        .collect();
    for e in &staged {
        review::fold(&mut next, e)?;
    }

    // Commit, minting each entry's global seq; `pre_commit` shares the tx.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    pre_commit(&tx)?;
    let mut applied = Vec::with_capacity(staged.len());
    for e in staged {
        let seq = db::append_log(&tx, change_id, e.idx, e.kind.as_str(), &e.payload, &now)?;
        applied.push(review::Entry { seq, ..e });
    }
    tx.commit()?;

    // Install the validated projection after the durable commit, then release
    // the lock before publishing so readers unblock.
    *proj = next;
    drop(proj);
    // Publish to live followers only after the durable commit + fold, so a
    // follower reconciling against its backlog never sees a half-applied entry.
    for e in &applied {
        entry.publish(StreamMsg::Entry(super::views::log_entry_view(change_id, e)));
    }
    Ok(applied)
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

/// `SQLITE_BUSY/LOCKED` anywhere in an error chain: cross-change write
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
