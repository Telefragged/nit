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
use axum::http::StatusCode;
use futures_util::StreamExt;
use nit_types::attachments::UploadedAttachment;
use nit_types::domain::AttachmentHash;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::db;

use super::{AppState, Error, with_conn};

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
