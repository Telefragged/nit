use std::collections::BTreeSet;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::http::header;
use axum::response::IntoResponse;
use nit_document::wire::{
    CreateDocument, DocumentList, DocumentNumber, DocumentQuery, DocumentSent,
    DocumentSubscription, DocumentSummary, update_frame,
};
use nit_document::{Document, Reference, Update, Version};
use nit_types::domain::AttachmentHash;
use rusqlite::{Connection, TransactionBehavior};

use crate::db;

use super::stream::{follow, text_frame};
use super::{AppJson, AppPath, AppQuery, AppState, Error, with_conn};

fn not_found(number: DocumentNumber) -> Error {
    Error::not_found(format!("document {number} not found"))
}

fn summary_of(conn: &Connection, number: DocumentNumber) -> Result<DocumentSummary, Error> {
    db::get_document(conn, number)?.ok_or_else(|| not_found(number))
}

fn snapshot_of(conn: &Connection, number: DocumentNumber) -> Result<Vec<u8>, Error> {
    db::document_snapshot(conn, number)?.ok_or_else(|| not_found(number))
}

fn load(conn: &Connection, number: DocumentNumber) -> Result<Document, Error> {
    Document::load(&snapshot_of(conn, number)?)
        .map_err(|e| Error::internal(format!("document {number}: {e}")))
}

fn uploads(document: &Document) -> Vec<AttachmentHash> {
    nit_document::references(&document.text())
        .into_iter()
        .filter_map(|placed| match placed.reference {
            Reference::Upload { hash, .. } => Some(hash),
            Reference::Change { .. } | Reference::File { .. } => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// `POST /api/documents` — creates an empty document.
#[utoipa::path(post, path = "/api/documents", request_body = CreateDocument, responses((status = 200, body = DocumentSummary)))]
pub(super) async fn create_document(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<CreateDocument>,
) -> Result<Json<DocumentSummary>, Error> {
    with_conn(state.pool(), move |conn| {
        let document = Document::new();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let number = db::insert_document(
            &tx,
            &req.title,
            &req.tags,
            &document.save(),
            &document.version(),
        )?;
        state.commit_and_signal_documents(tx)?;
        Ok(Json(summary_of(conn, number)?))
    })
    .await
}

/// `GET /api/documents` — the documents that the query selects.
#[utoipa::path(get, path = "/api/documents", params(DocumentQuery), responses((status = 200, body = DocumentList)))]
pub(super) async fn list_documents(
    State(state): State<Arc<AppState>>,
    AppQuery(query): AppQuery<DocumentQuery>,
) -> Result<Json<DocumentList>, Error> {
    with_conn(state.pool(), move |conn| {
        Ok(Json(DocumentList {
            documents: db::documents(conn, &query)?,
        }))
    })
    .await
}

/// `GET /api/documents/{number}` — a document without its text.
/// Clients load the text from the snapshot.
#[utoipa::path(get, path = "/api/documents/{number}", params(("number" = DocumentNumber, Path)), responses((status = 200, body = DocumentSummary)))]
pub(super) async fn get_document(
    State(state): State<Arc<AppState>>,
    AppPath(number): AppPath<DocumentNumber>,
) -> Result<Json<DocumentSummary>, Error> {
    with_conn(state.pool(), move |conn| {
        Ok(Json(summary_of(conn, number)?))
    })
    .await
}

/// `GET /api/documents/{number}/snapshot` — the bytes that
/// [`Document::load`] accepts: the text with its full history.
#[utoipa::path(get, path = "/api/documents/{number}/snapshot", params(("number" = DocumentNumber, Path)), responses((status = 200, body = Vec<u8>, content_type = "application/octet-stream")))]
pub(super) async fn snapshot(
    State(state): State<Arc<AppState>>,
    AppPath(number): AppPath<DocumentNumber>,
) -> Result<impl IntoResponse, Error> {
    let snapshot = with_conn(state.pool(), move |conn| snapshot_of(conn, number)).await?;
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        snapshot,
    ))
}

/// `POST /api/documents/{number}/updates` — merges the edits of a
/// client replica. The body is the bytes of an [`Update`].
///
/// This is the only route that changes the text of a document. Bytes
/// that are not an update are a 400. An upload that the store lacks is
/// a 404: upload it first. A merge that breaks an invariant of the
/// document is a 500, and the stored document stays as it was.
#[utoipa::path(post, path = "/api/documents/{number}/updates", params(("number" = DocumentNumber, Path)), request_body(content = Vec<u8>, content_type = "application/octet-stream"), responses((status = 200, body = DocumentSummary)))]
pub(super) async fn merge_update(
    State(state): State<Arc<AppState>>,
    AppPath(number): AppPath<DocumentNumber>,
    body: Bytes,
) -> Result<Json<DocumentSummary>, Error> {
    let update = Update::from_bytes(body.to_vec());
    with_conn(state.pool(), move |conn| {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut document = load(&tx, number)?;
        document
            .merge(&update)
            .map_err(|e| Error::bad_request(e.to_string()))?;
        let uploads = uploads(&document);
        for hash in &uploads {
            if !db::attachment_exists(&tx, hash)? {
                return Err(Error::not_found(format!(
                    "no attachment {hash}: upload it first"
                )));
            }
        }
        db::store_document(&tx, number, &document.save(), &document.version(), &uploads)?;
        state.commit_and_signal_documents(tx)?;
        Ok(Json(summary_of(conn, number)?))
    })
    .await
}

/// `POST /api/documents/{number}/send` — reviewers send a version of
/// the document to authors. The body is that [`Version`].
///
/// The stream reports the version to each client that follows the sent
/// versions. A version with an edit that the server lacks is a 400.
#[utoipa::path(post, path = "/api/documents/{number}/send", params(("number" = DocumentNumber, Path)), request_body = String, responses((status = 200, body = DocumentSummary)))]
pub(super) async fn send_document(
    State(state): State<Arc<AppState>>,
    AppPath(number): AppPath<DocumentNumber>,
    AppJson(version): AppJson<Version>,
) -> Result<Json<DocumentSummary>, Error> {
    with_conn(state.pool(), move |conn| {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !load(&tx, number)?.contains(&version) {
            return Err(Error::bad_request(
                "the version has an edit that the server lacks",
            ));
        }
        db::set_document_sent(&tx, number, &version)?;
        state.commit_and_signal_documents(tx)?;
        Ok(Json(summary_of(conn, number)?))
    })
    .await
}

/// `WS /api/documents/stream` — the edits and the sent versions of
/// documents. [`DocumentSubscription`] documents the contract.
#[utoipa::path(get, path = "/api/documents/stream", responses((status = 101, description = "A websocket of update frames and `DocumentSent` frames")))]
pub(super) async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let written = state.documents_watch();
        follow(socket, state, written, frames)
    })
}

async fn frames(
    state: Arc<AppState>,
    mut subscription: DocumentSubscription,
) -> Result<(Vec<Message>, DocumentSubscription), Error> {
    with_conn(state.pool(), move |conn| {
        let mut frames = Vec::new();
        for (number, have) in &mut subscription.updates {
            let Some(row) = db::get_document(conn, *number)? else {
                continue;
            };
            if row.version == *have {
                continue;
            }
            let update = load(conn, *number)?.updates_since(have);
            frames.push(Message::Binary(update_frame(*number, &update).into()));
            *have = row.version;
        }
        if let Some(query) = &subscription.sent {
            for row in db::documents(conn, query)? {
                let Some(version) = row.sent else { continue };
                if subscription.seen.get(&row.number) != Some(&version) {
                    subscription.seen.insert(row.number, version.clone());
                    frames.push(text_frame(&DocumentSent {
                        document: row.number,
                        version,
                    })?);
                }
            }
        }
        Ok((frames, subscription))
    })
    .await
}
