//! HTTP handlers mirroring `lore-ui::backend::Backend` trait methods.
//!
//! Each endpoint is `POST /api/<method>` with a JSON body whose fields map
//! 1:1 to the method's parameters, and a JSON response that's the method's
//! return value. Errors are always JSON too: the wrapper [`ApiError`]
//! serializes [`BackendError`] from `lore-core` as
//! `{ "code": "...", "message": "..." }` with a matching HTTP status
//! (`NotFound`/`RouteNotFound` → 404, `InvalidInput` → 400, `Internal`
//! → 500). The shape stays identical whether the failure comes from a
//! handler's `Result::Err` (DB lookup, base64 decode), the route fallback
//! (no such endpoint), or axum's `JsonRejection` (malformed body, missing
//! content-type) — so the future `HttpBackend` deserializes one type
//! across the board.
//!
//! Connection model mirrors `LocalBackend`: each handler opens a per-request
//! connection via `db::open_existing` against the path held in `AppState`.
//! Bootstrap (migrations + seed) happens once at server start in `main.rs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use lore_core::db::{
    self, ArchiveOutcome, AttachmentRow, ClassificationRule, FileRow, FolderRow,
    InsertAttachmentOutcome, InsertFileOutcome, NoteData, NoteRow, PageRef, SnapshotContent,
    SnapshotMeta, SpaceRow, SpaceStats, TrashItem, WebPageDetail, WebPageRow,
};
use lore_core::error::{BackendError, ErrorCode};
use lore_core::search;

#[derive(Clone)]
pub struct AppState {
    pub db_path: PathBuf,
    pub static_dir: PathBuf,
}

pub type AppStateExt = State<Arc<AppState>>;

fn conn(state: &AppState) -> Result<rusqlite::Connection, BackendError> {
    db::open_existing(&state.db_path).map_err(BackendError::from)
}

// ---- BackendError → HTTP response ----
//
// `BackendError` lives in `lore-core` for both server and client to share;
// the `IntoResponse` impl is server-side wire glue (axum-specific), so it
// stays here in a wrapper rather than polluting `lore-core` with axum deps.

pub struct ApiError(pub BackendError);

impl From<BackendError> for ApiError {
    fn from(e: BackendError) -> Self {
        Self(e)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(BackendError::from(e))
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        // Same `QueryReturnedNoRows` → `NotFound` mapping as the
        // `anyhow::Error` path — see `BackendError::from<rusqlite::Error>`
        // in `lore-core`. Used by handlers that hit rusqlite directly
        // (e.g. `db_schema_version`'s raw `PRAGMA user_version`).
        Self(BackendError::from(e))
    }
}

fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::RouteNotFound | ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (status_for(self.0.code), Json(self.0)).into_response()
    }
}

/// Fallback for routes the server doesn't expose. Returns a JSON body
/// with `code: "route_not_found"` so the client can distinguish "the API
/// shape changed" from "the requested entity is gone".
pub async fn route_not_found() -> ApiError {
    ApiError(BackendError::route_not_found(
        "no such API endpoint on this server",
    ))
}

/// Serves `index.html` with `Cache-Control: no-store` so the browser always
/// fetches a fresh copy. The JS/WASM assets already carry content hashes in
/// their filenames (from `dx build`), so only the HTML entry point needs
/// this — without it the browser may load a stale `index.html` that
/// references old bundle hashes after a redeploy.
pub async fn serve_index(State(state): AppStateExt) -> Response {
    let path = state.static_dir.join("index.html");
    match tokio::fs::read(path).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/html; charset=utf-8")
            .header("Cache-Control", "no-store")
            .body(axum::body::Body::from(bytes))
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::from("index.html not found"))
            .unwrap(),
    }
}

/// JSON request extractor that converts axum's `JsonRejection` (missing
/// content-type, malformed JSON, deserialization failure, …) into an
/// `ApiError` with `code: "invalid_input"` so the wire format stays
/// uniform — every server-emitted error is a `BackendError` JSON.
pub struct JsonReq<T>(pub T);

impl<S, T> FromRequest<S> for JsonReq<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(req, state)
            .await
            .map(|Json(v)| JsonReq(v))
            .map_err(|rej: JsonRejection| ApiError(BackendError::invalid_input(rej.body_text())))
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// ---- Bootstrap ----

pub async fn get_revision(State(s): AppStateExt) -> ApiResult<i64> {
    Ok(Json(db::get_revision(&conn(&s)?).map_err(ApiError::from)?))
}

pub async fn db_schema_version(State(s): AppStateExt) -> ApiResult<u32> {
    // Raw open — no `open_existing` pragmas — so an under-us migration
    // bump doesn't trip the WAL/foreign-key apply step.
    let c = rusqlite::Connection::open(&s.db_path).map_err(ApiError::from)?;
    let v: u32 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(ApiError::from)?;
    Ok(Json(v))
}

// ---- Spaces ----

pub async fn list_spaces(State(s): AppStateExt) -> ApiResult<Vec<SpaceRow>> {
    Ok(Json(db::list_spaces(&conn(&s)?).map_err(ApiError::from)?))
}

pub async fn list_all_spaces(State(s): AppStateExt) -> ApiResult<Vec<SpaceRow>> {
    Ok(Json(
        db::list_all_spaces(&conn(&s)?).map_err(ApiError::from)?,
    ))
}

pub async fn get_active_space(State(s): AppStateExt) -> ApiResult<SpaceRow> {
    db::get_active_space(&conn(&s)?)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct SpaceIdReq {
    pub space_id: i64,
}

pub async fn space_stats(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<SpaceStats> {
    db::space_stats(&conn(&s)?, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn touch_space(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<()> {
    db::touch_space(&conn(&s)?, req.space_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct CreateSpaceReq {
    pub name: String,
}

pub async fn create_space(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<CreateSpaceReq>,
) -> ApiResult<i64> {
    db::insert_space(&conn(&s)?, &req.name)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct RenameSpaceReq {
    pub space_id: i64,
    pub name: String,
}

pub async fn rename_space(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<RenameSpaceReq>,
) -> ApiResult<()> {
    db::rename_space(&conn(&s)?, req.space_id, &req.name).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn trash_space(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<()> {
    db::trash_space(&conn(&s)?, req.space_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn restore_space(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<()> {
    db::restore_space(&conn(&s)?, req.space_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn delete_space_permanent(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<()> {
    db::delete_space_permanent(&conn(&s)?, req.space_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

// ---- Folders ----

pub async fn list_folders(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<Vec<FolderRow>> {
    db::list_folders(&conn(&s)?, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn folder_note_counts(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<HashMap<i64, i64>> {
    db::folder_note_counts(&conn(&s)?, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct CreateFolderReq {
    pub name: String,
    pub parent_id: Option<i64>,
    pub space_id: i64,
}

pub async fn create_folder(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<CreateFolderReq>,
) -> ApiResult<i64> {
    db::insert_folder(&conn(&s)?, &req.name, req.parent_id, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct RenameFolderReq {
    pub folder_id: i64,
    pub name: String,
}

pub async fn rename_folder(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<RenameFolderReq>,
) -> ApiResult<()> {
    db::rename_folder(&conn(&s)?, req.folder_id, &req.name).map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct FolderIdReq {
    pub folder_id: i64,
}

pub async fn delete_folder(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<FolderIdReq>,
) -> ApiResult<()> {
    db::delete_folder(&conn(&s)?, req.folder_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

// ---- Notes ----

#[derive(Deserialize)]
pub struct ListNotesReq {
    pub folder_id: Option<i64>,
    pub space_id: i64,
}

pub async fn list_notes(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<ListNotesReq>,
) -> ApiResult<Vec<NoteRow>> {
    db::list_notes(&conn(&s)?, req.folder_id, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn list_note_ids_ordered(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<ListNotesReq>,
) -> ApiResult<Vec<i64>> {
    db::list_note_ids_ordered(&conn(&s)?, req.folder_id, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct NoteIdReq {
    pub note_id: i64,
}

pub async fn get_note(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<NoteIdReq>,
) -> ApiResult<NoteData> {
    db::get_note(&conn(&s)?, req.note_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct CreateNoteReq {
    pub title: String,
    pub body: String,
    pub folder_id: Option<i64>,
    pub space_id: i64,
}

pub async fn create_note(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<CreateNoteReq>,
) -> ApiResult<i64> {
    db::insert_note(
        &conn(&s)?,
        &req.title,
        &req.body,
        req.folder_id,
        req.space_id,
    )
    .map(Json)
    .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct UpdateNoteReq {
    pub note_id: i64,
    pub title: String,
    pub body: String,
}

pub async fn update_note(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<UpdateNoteReq>,
) -> ApiResult<()> {
    db::update_note(&conn(&s)?, req.note_id, &req.title, &req.body).map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct MoveNoteReq {
    pub note_id: i64,
    pub folder_id: Option<i64>,
}

pub async fn move_note(State(s): AppStateExt, JsonReq(req): JsonReq<MoveNoteReq>) -> ApiResult<()> {
    db::move_note_to_folder(&conn(&s)?, req.note_id, req.folder_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn trash_note(State(s): AppStateExt, JsonReq(req): JsonReq<NoteIdReq>) -> ApiResult<()> {
    db::trash_note(&conn(&s)?, req.note_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn restore_note(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<NoteIdReq>,
) -> ApiResult<()> {
    db::restore_note_safe(&conn(&s)?, req.note_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn delete_note_permanent(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<NoteIdReq>,
) -> ApiResult<()> {
    db::delete_note_permanent(&conn(&s)?, req.note_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct FindNotesReferencingUrlReq {
    pub url: String,
    pub space_id: i64,
}

pub async fn find_notes_referencing_url(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<FindNotesReferencingUrlReq>,
) -> ApiResult<Vec<(i64, String)>> {
    db::find_notes_referencing_url(&conn(&s)?, &req.url, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

// ---- Pages ----

#[derive(Deserialize)]
pub struct ListPagesReq {
    pub space_id: i64,
    pub limit: usize,
}

pub async fn list_pages(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<ListPagesReq>,
) -> ApiResult<Vec<WebPageRow>> {
    db::list_pages(&conn(&s)?, req.space_id, req.limit)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn list_page_ids_ordered(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<ListPagesReq>,
) -> ApiResult<Vec<i64>> {
    db::list_page_ids_ordered(&conn(&s)?, req.space_id, req.limit)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct PageIdReq {
    pub page_id: i64,
}

pub async fn get_page(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<PageIdReq>,
) -> ApiResult<WebPageDetail> {
    // `WebPageDetail.snapshot.screenshot` is `Option<Vec<u8>>` with a
    // `serde_b64::opt_vec` helper, so this serializes the PNG bytes as a
    // base64 string automatically.
    db::get_page(&conn(&s)?, req.page_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct ArchiveUrlReq {
    pub raw_url: String,
    pub space_id: Option<i64>,
    pub title: Option<String>,
    pub source: Option<String>,
}

pub async fn archive_url(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<ArchiveUrlReq>,
) -> ApiResult<ArchiveOutcome> {
    db::archive_url(
        &conn(&s)?,
        &req.raw_url,
        req.space_id,
        req.title.as_deref(),
        req.source.as_deref(),
    )
    .map(Json)
    .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct AutoArchiveReq {
    pub text: String,
    pub space_id: i64,
}

pub async fn auto_archive_from_text(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<AutoArchiveReq>,
) -> ApiResult<usize> {
    db::auto_archive_from_text(&conn(&s)?, &req.text, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct CheckUrlsReq {
    pub urls: Vec<String>,
}

pub async fn check_urls_status(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<CheckUrlsReq>,
) -> ApiResult<HashMap<String, String>> {
    db::check_urls_status(&conn(&s)?, &req.urls)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn trash_page(State(s): AppStateExt, JsonReq(req): JsonReq<PageIdReq>) -> ApiResult<()> {
    db::trash_page(&conn(&s)?, req.page_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn restore_page(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<PageIdReq>,
) -> ApiResult<()> {
    db::restore_page(&conn(&s)?, req.page_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn delete_page_permanent(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<PageIdReq>,
) -> ApiResult<()> {
    db::delete_page(&conn(&s)?, req.page_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct UpdatePageStatusReq {
    pub page_id: i64,
    pub status: String,
}

pub async fn update_page_status(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<UpdatePageStatusReq>,
) -> ApiResult<()> {
    db::update_status(&conn(&s)?, req.page_id, &req.status).map_err(ApiError::from)?;
    Ok(Json(()))
}

// ---- Page versions ----

pub async fn list_page_versions(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<PageIdReq>,
) -> ApiResult<Vec<SnapshotMeta>> {
    db::list_page_versions(&conn(&s)?, req.page_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct SnapshotIdReq {
    pub snapshot_id: i64,
}

pub async fn get_page_version(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SnapshotIdReq>,
) -> ApiResult<SnapshotContent> {
    db::get_page_version(&conn(&s)?, req.snapshot_id)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn delete_page_version(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SnapshotIdReq>,
) -> ApiResult<()> {
    db::delete_page_version(&conn(&s)?, req.snapshot_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn request_reachive(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<PageIdReq>,
) -> ApiResult<()> {
    db::request_reachive(&conn(&s)?, req.page_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

// ---- Files ----

pub async fn list_files(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<Vec<FileRow>> {
    db::list_files(&conn(&s)?, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct FileIdReq {
    pub file_id: i64,
}

pub async fn get_file(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<FileIdReq>,
) -> ApiResult<FileRow> {
    db::get_file(&conn(&s)?, req.file_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Serialize)]
pub struct FileBytesDto {
    pub mime_type: Option<String>,
    pub data_b64: String,
}

pub async fn get_file_data(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<FileIdReq>,
) -> ApiResult<FileBytesDto> {
    let (mime, bytes) = db::get_file_data(&conn(&s)?, req.file_id).map_err(ApiError::from)?;
    Ok(Json(FileBytesDto {
        mime_type: mime,
        data_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    }))
}

#[derive(Deserialize)]
pub struct InsertFileReq {
    pub name: String,
    pub mime_type: Option<String>,
    pub data_b64: String,
    pub space_id: i64,
}

pub async fn insert_file(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<InsertFileReq>,
) -> ApiResult<(i64, InsertFileOutcome)> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.data_b64.as_bytes())
        .map_err(|e| ApiError(BackendError::invalid_input(format!("data_b64: {}", e))))?;
    db::insert_file(
        &conn(&s)?,
        &req.name,
        req.mime_type.as_deref(),
        &bytes,
        req.space_id,
    )
    .map(Json)
    .map_err(ApiError::from)
}

pub async fn trash_file(State(s): AppStateExt, JsonReq(req): JsonReq<FileIdReq>) -> ApiResult<()> {
    db::trash_file(&conn(&s)?, req.file_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn restore_file(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<FileIdReq>,
) -> ApiResult<()> {
    db::restore_file(&conn(&s)?, req.file_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

pub async fn delete_file_permanent(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<FileIdReq>,
) -> ApiResult<()> {
    db::delete_file_permanent(&conn(&s)?, req.file_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

// ---- Attachments ----

pub async fn list_attachments(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<NoteIdReq>,
) -> ApiResult<Vec<AttachmentRow>> {
    db::list_attachments(&conn(&s)?, req.note_id)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn list_removed_attachments(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<NoteIdReq>,
) -> ApiResult<Vec<AttachmentRow>> {
    db::list_removed_attachments(&conn(&s)?, req.note_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct AttachmentIdReq {
    pub attachment_id: i64,
}

pub async fn get_attachment(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<AttachmentIdReq>,
) -> ApiResult<AttachmentRow> {
    db::get_attachment(&conn(&s)?, req.attachment_id)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Serialize)]
pub struct AttachmentBytesDto {
    pub mime_type: String,
    pub data_b64: String,
}

pub async fn get_attachment_data(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<AttachmentIdReq>,
) -> ApiResult<AttachmentBytesDto> {
    let (mime, bytes) =
        db::get_attachment_data(&conn(&s)?, req.attachment_id).map_err(ApiError::from)?;
    Ok(Json(AttachmentBytesDto {
        mime_type: mime,
        data_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    }))
}

#[derive(Deserialize)]
pub struct InsertAttachmentReq {
    pub note_id: i64,
    pub name: String,
    pub mime_type: String,
    pub data_b64: String,
}

pub async fn insert_attachment(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<InsertAttachmentReq>,
) -> ApiResult<(i64, InsertAttachmentOutcome)> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.data_b64.as_bytes())
        .map_err(|e| ApiError(BackendError::invalid_input(format!("data_b64: {}", e))))?;
    db::insert_attachment(&conn(&s)?, req.note_id, &req.name, &req.mime_type, &bytes)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct CleanupOrphanedAttachmentsReq {
    pub note_id: i64,
    pub used_ids: Vec<i64>,
}

pub async fn cleanup_orphaned_attachments(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<CleanupOrphanedAttachmentsReq>,
) -> ApiResult<usize> {
    db::cleanup_orphaned_attachments(&conn(&s)?, req.note_id, &req.used_ids)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn restore_attachment(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<AttachmentIdReq>,
) -> ApiResult<()> {
    db::restore_attachment(&conn(&s)?, req.attachment_id).map_err(ApiError::from)?;
    Ok(Json(()))
}

// ---- Trash ----

pub async fn list_trash(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<Vec<TrashItem>> {
    db::list_trash(&conn(&s)?, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn trash_count(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SpaceIdReq>,
) -> ApiResult<i64> {
    db::trash_count(&conn(&s)?, req.space_id)
        .map(Json)
        .map_err(ApiError::from)
}

// ---- Activity ----

#[derive(Deserialize)]
pub struct ActivityByDayReq {
    pub space_id: i64,
    pub days: i64,
}

pub async fn activity_by_day(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<ActivityByDayReq>,
) -> ApiResult<Vec<(String, i64)>> {
    db::activity_by_day(&conn(&s)?, req.space_id, req.days)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Deserialize)]
pub struct ActivityForDayReq {
    pub space_id: i64,
    pub day: String,
}

pub async fn activity_for_day(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<ActivityForDayReq>,
) -> ApiResult<(Vec<NoteRow>, Vec<PageRef>)> {
    db::activity_for_day(&conn(&s)?, req.space_id, &req.day)
        .map(Json)
        .map_err(ApiError::from)
}

// ---- Classification rules ----

pub async fn load_rules(State(s): AppStateExt) -> ApiResult<Vec<ClassificationRule>> {
    db::load_rules(&conn(&s)?).map(Json).map_err(ApiError::from)
}

// ---- FTS5 search ----

#[derive(Deserialize)]
pub struct SearchReq {
    pub query: String,
    pub space_id: i64,
    pub limit: usize,
}

pub async fn search_pages_brief(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SearchReq>,
) -> ApiResult<Vec<WebPageRow>> {
    search::search_web_pages_brief(&conn(&s)?, &req.query, req.space_id, req.limit)
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn search_notes(
    State(s): AppStateExt,
    JsonReq(req): JsonReq<SearchReq>,
) -> ApiResult<Vec<NoteRow>> {
    search::search_notes(&conn(&s)?, &req.query, req.space_id, req.limit)
        .map(Json)
        .map_err(ApiError::from)
}

// ---- Raw blob endpoints ----
//
// `GET /api/files/:id/raw` and `GET /api/attachments/:id/raw` return the
// stored bytes with a `Content-Disposition: attachment` header so the
// browser triggers a download. Web build's "Save" button is just an
// `<a href="...raw" download>` anchor; the desktop variant still uses
// `rfd::AsyncFileDialog`. Filenames are echoed back in the header
// (plain-quoted; non-ASCII is left to the browser's best-effort
// decoding for the W3 cut — RFC 5987 encoding is a later polish).

pub async fn file_raw(
    State(s): AppStateExt,
    Path(file_id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let c = conn(&s)?;
    let file = db::get_file(&c, file_id).map_err(ApiError::from)?;
    let (mime, bytes) = db::get_file_data(&c, file_id).map_err(ApiError::from)?;
    let mime = mime.unwrap_or_else(|| "application/octet-stream".to_string());
    Ok((
        [
            (CONTENT_TYPE, mime),
            (
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", file.name),
            ),
        ],
        bytes,
    ))
}

pub async fn attachment_raw(
    State(s): AppStateExt,
    Path(attachment_id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let c = conn(&s)?;
    let att = db::get_attachment(&c, attachment_id).map_err(ApiError::from)?;
    let (mime, bytes) = db::get_attachment_data(&c, attachment_id).map_err(ApiError::from)?;
    Ok((
        [
            (CONTENT_TYPE, mime),
            (
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", att.name),
            ),
        ],
        bytes,
    ))
}
