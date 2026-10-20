//! Attachments: the blob store and the routes over it.
//!
//! A blob is a file under the store directory, named by its
//! [`AttachmentHash`]. An upload streams into `.tmp/` first and moves into
//! place only once the whole body is hashed, so a blob file is never
//! partial.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use futures_util::StreamExt;
use jiff::Timestamp;
use nit_types::attachments::UploadedAttachment;
use nit_types::changes::ChangeDetail;
use nit_types::domain::{AttachPayload, AttachmentHash, ChangeNumber, LogPayload};
use rusqlite::TransactionBehavior;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;

use crate::db;

use super::{AppJson, AppPath, AppState, Error, append, views, with_conn};

/// The largest upload the store accepts, in bytes.
const MAX_SIZE: u64 = 64 * 1024 * 1024;

/// The directory of blob files, one per [`AttachmentHash`].
pub struct Blobs {
    dir: PathBuf,
    next_tmp: AtomicU64,
    /// Held while a file moves into or out of the blob directory, and
    /// across the database write that records the move.
    ///
    /// Without it, a collection could delete a blob file that an upload
    /// of the same bytes had just moved into place. The lock is local to
    /// the process, as the sqlite database is.
    moves: Mutex<()>,
}

impl Blobs {
    /// Opens the store at `dir`, creating it and its `.tmp/` if needed.
    ///
    /// # Errors
    ///
    /// When a directory cannot be created.
    pub fn open(dir: PathBuf) -> std::io::Result<Blobs> {
        std::fs::create_dir_all(dir.join(".tmp"))?;
        Ok(Blobs {
            dir,
            next_tmp: AtomicU64::new(0),
            moves: Mutex::new(()),
        })
    }

    fn path(&self, hash: &AttachmentHash) -> PathBuf {
        self.dir.join(hash.as_str())
    }

    /// Deletes the blobs that no change links to, once they are older than
    /// `cutoff`.
    ///
    /// It deletes the temp files of uploads that never finished, the
    /// blob rows that no relation names together with their files, and
    /// the blob files that have no row. A file without a row is what a
    /// crash between a row's deletion and its file's deletion leaves.
    ///
    /// # Errors
    ///
    /// When the database or the store directory fails.
    pub(super) async fn collect(&self, state: &AppState, cutoff: Timestamp) -> anyhow::Result<()> {
        let older = std::time::SystemTime::from(cutoff);
        for (path, _) in files_older_than(&self.dir.join(".tmp"), older)? {
            remove(&path);
        }
        let _moving = self.moves.lock().await;
        let deleted = with_conn(state.pool(), move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let deleted = db::delete_unrelated_attachments(&tx, cutoff)?;
            tx.commit()?;
            Ok(deleted)
        })
        .await
        .map_err(|e| anyhow::anyhow!(e.message))?;
        for hash in &deleted {
            remove(&self.path(hash));
        }
        let files: Vec<(PathBuf, AttachmentHash)> = files_older_than(&self.dir, older)?
            .into_iter()
            .filter_map(|(path, name)| Some((path, AttachmentHash::new(name).ok()?)))
            .collect();
        let rowless = with_conn(state.pool(), move |conn| {
            let mut rowless = Vec::new();
            for (path, hash) in files {
                if !db::attachment_exists(conn, &hash)? {
                    rowless.push(path);
                }
            }
            Ok(rowless)
        })
        .await
        .map_err(|e| anyhow::anyhow!(e.message))?;
        for path in rowless {
            remove(&path);
        }
        Ok(())
    }

    /// Streams `body` into a temp file, hashing it on the way.
    async fn stage(&self, body: Body) -> Result<(Staged, AttachmentHash), Error> {
        let n = self.next_tmp.fetch_add(1, Ordering::Relaxed);
        let path = self
            .dir
            .join(".tmp")
            .join(format!("{}-{n}", std::process::id()));
        let file = tokio::fs::File::create(&path).await.map_err(io_error)?;
        let staged = Staged(Some(path));
        let mut file = tokio::io::BufWriter::with_capacity(1 << 20, file);
        let mut hasher = blake3::Hasher::new();
        let mut size = 0;
        let mut chunks = body.into_data_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk =
                chunk.map_err(|e| Error::bad_request(format!("reading the upload: {e}")))?;
            size += chunk.len() as u64;
            if size > MAX_SIZE {
                return Err(Error {
                    status: StatusCode::PAYLOAD_TOO_LARGE,
                    message: format!("an attachment is at most {MAX_SIZE} bytes"),
                });
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(io_error)?;
        }
        file.flush().await.map_err(io_error)?;
        file.get_ref().sync_all().await.map_err(io_error)?;
        let hash = AttachmentHash::new(hasher.finalize().to_hex().to_string())
            .map_err(|e| Error::internal(e.to_string()))?;
        Ok((staged, hash))
    }
}

/// An upload in `.tmp/`, deleted on drop unless it moved into place.
struct Staged(Option<PathBuf>);

impl Staged {
    fn install(mut self, to: &Path) -> Result<(), Error> {
        let from = self.0.take().expect("a staged file stays until installed");
        std::fs::rename(from, to).map_err(io_error)
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The files in `dir` last modified before `older`, with their names.
fn files_older_than(
    dir: &Path,
    older: std::time::SystemTime,
) -> std::io::Result<Vec<(PathBuf, String)>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file()
            && meta.modified()? < older
            && let Ok(name) = entry.file_name().into_string()
        {
            files.push((entry.path(), name));
        }
    }
    Ok(files)
}

fn remove(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("deleting {}: {e}", path.display());
    }
}

fn io_error(e: std::io::Error) -> Error {
    anyhow::Error::from(e).context("attachment store").into()
}

/// `PUT /api/attachments` — stores the request body as a blob.
///
/// Storing bytes that the store already holds succeeds and refreshes the
/// blob's age, so a collection does not delete it before an attach names
/// it. A body over the size limit is a 413.
#[utoipa::path(put, path = "/api/attachments", request_body(content = Vec<u8>, content_type = "application/octet-stream"), responses((status = 200, body = UploadedAttachment)))]
pub(super) async fn upload(
    State(state): State<Arc<AppState>>,
    body: Body,
) -> Result<Json<UploadedAttachment>, Error> {
    let blobs = state.blobs();
    let (staged, hash) = blobs.stage(body).await?;
    let _moving = blobs.moves.lock().await;
    let path = blobs.path(&hash);
    // A blob of these bytes is already in place, and the staged copy is
    // deleted on drop.
    if !tokio::fs::try_exists(&path).await.map_err(io_error)? {
        staged.install(&path)?;
    }
    let stored = hash.clone();
    with_conn(state.pool(), move |conn| {
        db::upsert_attachment(conn, &stored, &db::now_rfc3339())?;
        Ok(())
    })
    .await?;
    Ok(Json(UploadedAttachment { hash }))
}

/// `POST /api/changes/{id}/attachments` — puts a stored blob on a revision.
///
/// The request is the entry it appends.
///
/// Any revision of the change takes an attachment, not only the latest.
/// A name the revision already holds with the same hash appends nothing.
/// A revision or a blob that does not exist is a 404, and so is a blob
/// that a collection deleted before the attach: upload it again.
#[utoipa::path(post, path = "/api/changes/{id}/attachments", params(("id" = ChangeNumber, Path)), request_body = AttachPayload, responses((status = 200, body = ChangeDetail)))]
pub(super) async fn attach(
    State(state): State<Arc<AppState>>,
    AppPath(id): AppPath<ChangeNumber>,
    AppJson(req): AppJson<AttachPayload>,
) -> Result<Json<ChangeDetail>, Error> {
    with_conn(state.pool(), move |conn| {
        let appended = append(&state, conn, id, |change, tx| {
            let Some(revision) = change.revision(req.revision) else {
                return Err(Error::not_found(format!(
                    "change {id} has no revision {}",
                    req.revision
                )));
            };
            if revision
                .attachments
                .iter()
                .any(|a| a.name == req.name && a.hash == req.hash)
            {
                return Ok(Vec::new());
            }
            if !db::attachment_exists(tx, &req.hash)? {
                return Err(Error::not_found(format!(
                    "no attachment {}: upload it first",
                    req.hash
                )));
            }
            db::relate_change_attachment(tx, id, &req.hash)?;
            Ok(vec![LogPayload::Attach(req)])
        })?;
        Ok(Json(views::build_change_detail(conn, &appended.change)?))
    })
    .await
}

/// `GET /api/attachments/{hash}/{name}` — the bytes of a blob.
///
/// `name` is a file name, typically the last segment of an
/// [`AttachmentName`](nit_types::domain::AttachmentName). It only selects
/// the `Content-Type`. The bytes for a hash never change, so the response
/// can be cached forever. The sandbox policy runs an HTML page in a
/// separate origin, so its scripts cannot call the API.
#[utoipa::path(get, path = "/api/attachments/{hash}/{name}", params(("hash" = AttachmentHash, Path), ("name" = String, Path)), responses((status = 200, body = Vec<u8>)))]
pub(super) async fn download(
    State(state): State<Arc<AppState>>,
    AppPath((hash, name)): AppPath<(AttachmentHash, String)>,
) -> Result<impl IntoResponse, Error> {
    let file = match tokio::fs::File::open(state.blobs().path(&hash)).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::not_found(format!("no attachment {hash}")));
        }
        Err(e) => return Err(io_error(e)),
    };
    let size = file.metadata().await.map_err(io_error)?.len();
    let mime = mime_guess::from_path(&name).first_or_octet_stream();
    let content_type = if mime.type_() == mime_guess::mime::TEXT {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_LENGTH, size.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (
                header::CONTENT_SECURITY_POLICY,
                "sandbox allow-scripts".to_string(),
            ),
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        Body::from_stream(ReaderStream::new(file)),
    ))
}
