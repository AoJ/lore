//! `HttpBackend` — talks to `lore-server`'s JSON API. Compiled only on
//! the `web` feature so its `gloo-net` dependency (browser-only) doesn't
//! leak into desktop builds.
//!
//! Every trait method serializes its parameters to JSON, POSTs them to
//! `<base_url>/<method_name>`, and deserializes the response. The error
//! path matches the server's wire format exactly: a non-2xx response is
//! parsed back into [`BackendError`] so the `code` field flows from
//! handler to view unchanged.

use async_trait::async_trait;
use base64::Engine;
use gloo_net::http::Request;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use lore_core::db::{
    ArchiveOutcome, AttachmentRow, ClassificationRule, FileRow, FolderRow, InsertAttachmentOutcome,
    InsertFileOutcome, NoteData, NoteRow, PageRef, SpaceRow, SpaceStats, TrashItem, WebPageDetail,
    WebPageRow,
};
use lore_core::error::BackendError;

use super::{Backend, Result};

#[derive(Clone)]
pub struct HttpBackend {
    /// Base URL of the API — usually `"/api"` (same-origin). All requests
    /// land at `<base_url>/<method_name>` with a JSON body.
    pub base_url: String,
}

impl HttpBackend {
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

/// One `POST /api/<method>` call. Serializes `req` as JSON body,
/// deserializes the response. On 4xx/5xx the body is parsed as
/// `BackendError` so the `code` field round-trips intact; if the body
/// isn't a valid `BackendError` JSON (shouldn't happen with our server
/// but a reverse proxy or 502 page might intercept), falls back to
/// `internal` with the status + raw text.
async fn call<Req: Serialize, Res: DeserializeOwned>(
    base_url: &str,
    method: &str,
    req: &Req,
) -> Result<Res> {
    let url = format!("{}/{}", base_url, method);
    let body = serde_json::to_string(req)
        .map_err(|e| BackendError::invalid_input(format!("serialize request: {}", e)))?;
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(|e| BackendError::internal(format!("build request: {}", e)))?
        .send()
        .await
        .map_err(|e| BackendError::internal(format!("network: {}", e)))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| BackendError::internal(format!("read response: {}", e)))?;

    if (200..300).contains(&status) {
        serde_json::from_str(&text).map_err(|e| {
            BackendError::internal(format!("deserialize {} response: {}: {}", method, e, text))
        })
    } else {
        match serde_json::from_str::<BackendError>(&text) {
            Ok(err) => Err(err),
            Err(_) => Err(BackendError::internal(format!("HTTP {}: {}", status, text))),
        }
    }
}

/// Empty request marker for methods that take no parameters. The server's
/// handler-signature pattern is `fn(State) -> ApiResult<T>` for those, so
/// any body is accepted; we send `null`.
#[derive(Serialize)]
struct EmptyReq;

// ---- DTOs that match server-side wrappers around `(Vec<u8>, ...)` returns ----

#[derive(Deserialize)]
struct FileBytesDto {
    mime_type: Option<String>,
    data_b64: String,
}

#[derive(Deserialize)]
struct AttachmentBytesDto {
    mime_type: String,
    data_b64: String,
}

fn decode_b64(s: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .map_err(|e| BackendError::internal(format!("decode b64: {}", e)))
}

fn encode_b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[async_trait(?Send)]
impl Backend for HttpBackend {
    // ---- Bootstrap ----

    async fn get_revision(&self) -> Result<i64> {
        call(&self.base_url, "get_revision", &EmptyReq).await
    }

    async fn db_schema_version(&self) -> Result<u32> {
        call(&self.base_url, "db_schema_version", &EmptyReq).await
    }

    // ---- Spaces ----

    async fn list_spaces(&self) -> Result<Vec<SpaceRow>> {
        call(&self.base_url, "list_spaces", &EmptyReq).await
    }

    async fn list_all_spaces(&self) -> Result<Vec<SpaceRow>> {
        call(&self.base_url, "list_all_spaces", &EmptyReq).await
    }

    async fn get_active_space(&self) -> Result<SpaceRow> {
        call(&self.base_url, "get_active_space", &EmptyReq).await
    }

    async fn space_stats(&self, space_id: i64) -> Result<SpaceStats> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "space_stats", &R { space_id }).await
    }

    async fn touch_space(&self, space_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "touch_space", &R { space_id }).await
    }

    async fn create_space(&self, name: &str) -> Result<i64> {
        #[derive(Serialize)]
        struct R<'a> {
            name: &'a str,
        }
        call(&self.base_url, "create_space", &R { name }).await
    }

    async fn rename_space(&self, space_id: i64, name: &str) -> Result<()> {
        #[derive(Serialize)]
        struct R<'a> {
            space_id: i64,
            name: &'a str,
        }
        call(&self.base_url, "rename_space", &R { space_id, name }).await
    }

    async fn trash_space(&self, space_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "trash_space", &R { space_id }).await
    }

    async fn restore_space(&self, space_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "restore_space", &R { space_id }).await
    }

    async fn delete_space_permanent(&self, space_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "delete_space_permanent", &R { space_id }).await
    }

    // ---- Folders ----

    async fn list_folders(&self, space_id: i64) -> Result<Vec<FolderRow>> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "list_folders", &R { space_id }).await
    }

    async fn folder_note_counts(&self, space_id: i64) -> Result<HashMap<i64, i64>> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "folder_note_counts", &R { space_id }).await
    }

    async fn create_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        space_id: i64,
    ) -> Result<i64> {
        #[derive(Serialize)]
        struct R<'a> {
            name: &'a str,
            parent_id: Option<i64>,
            space_id: i64,
        }
        call(
            &self.base_url,
            "create_folder",
            &R {
                name,
                parent_id,
                space_id,
            },
        )
        .await
    }

    async fn rename_folder(&self, folder_id: i64, name: &str) -> Result<()> {
        #[derive(Serialize)]
        struct R<'a> {
            folder_id: i64,
            name: &'a str,
        }
        call(&self.base_url, "rename_folder", &R { folder_id, name }).await
    }

    async fn delete_folder(&self, folder_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            folder_id: i64,
        }
        call(&self.base_url, "delete_folder", &R { folder_id }).await
    }

    // ---- Notes ----

    async fn list_notes(&self, folder_id: Option<i64>, space_id: i64) -> Result<Vec<NoteRow>> {
        #[derive(Serialize)]
        struct R {
            folder_id: Option<i64>,
            space_id: i64,
        }
        call(
            &self.base_url,
            "list_notes",
            &R {
                folder_id,
                space_id,
            },
        )
        .await
    }

    async fn list_note_ids_ordered(
        &self,
        folder_id: Option<i64>,
        space_id: i64,
    ) -> Result<Vec<i64>> {
        #[derive(Serialize)]
        struct R {
            folder_id: Option<i64>,
            space_id: i64,
        }
        call(
            &self.base_url,
            "list_note_ids_ordered",
            &R {
                folder_id,
                space_id,
            },
        )
        .await
    }

    async fn get_note(&self, note_id: i64) -> Result<NoteData> {
        #[derive(Serialize)]
        struct R {
            note_id: i64,
        }
        call(&self.base_url, "get_note", &R { note_id }).await
    }

    async fn create_note(
        &self,
        title: &str,
        body: &str,
        folder_id: Option<i64>,
        space_id: i64,
    ) -> Result<i64> {
        #[derive(Serialize)]
        struct R<'a> {
            title: &'a str,
            body: &'a str,
            folder_id: Option<i64>,
            space_id: i64,
        }
        call(
            &self.base_url,
            "create_note",
            &R {
                title,
                body,
                folder_id,
                space_id,
            },
        )
        .await
    }

    async fn update_note(&self, note_id: i64, title: &str, body: &str) -> Result<()> {
        #[derive(Serialize)]
        struct R<'a> {
            note_id: i64,
            title: &'a str,
            body: &'a str,
        }
        call(
            &self.base_url,
            "update_note",
            &R {
                note_id,
                title,
                body,
            },
        )
        .await
    }

    async fn move_note(&self, note_id: i64, folder_id: Option<i64>) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            note_id: i64,
            folder_id: Option<i64>,
        }
        call(&self.base_url, "move_note", &R { note_id, folder_id }).await
    }

    async fn trash_note(&self, note_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            note_id: i64,
        }
        call(&self.base_url, "trash_note", &R { note_id }).await
    }

    async fn restore_note(&self, note_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            note_id: i64,
        }
        call(&self.base_url, "restore_note", &R { note_id }).await
    }

    async fn delete_note_permanent(&self, note_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            note_id: i64,
        }
        call(&self.base_url, "delete_note_permanent", &R { note_id }).await
    }

    async fn find_notes_referencing_url(
        &self,
        url: &str,
        space_id: i64,
    ) -> Result<Vec<(i64, String)>> {
        #[derive(Serialize)]
        struct R<'a> {
            url: &'a str,
            space_id: i64,
        }
        call(
            &self.base_url,
            "find_notes_referencing_url",
            &R { url, space_id },
        )
        .await
    }

    // ---- Pages ----

    async fn list_pages(&self, space_id: i64, limit: usize) -> Result<Vec<WebPageRow>> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
            limit: usize,
        }
        call(&self.base_url, "list_pages", &R { space_id, limit }).await
    }

    async fn list_page_ids_ordered(&self, space_id: i64, limit: usize) -> Result<Vec<i64>> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
            limit: usize,
        }
        call(
            &self.base_url,
            "list_page_ids_ordered",
            &R { space_id, limit },
        )
        .await
    }

    async fn get_page(&self, page_id: i64) -> Result<WebPageDetail> {
        #[derive(Serialize)]
        struct R {
            page_id: i64,
        }
        call(&self.base_url, "get_page", &R { page_id }).await
    }

    async fn archive_url(
        &self,
        raw_url: &str,
        space_id: Option<i64>,
        title: Option<&str>,
        source: Option<&str>,
    ) -> Result<ArchiveOutcome> {
        #[derive(Serialize)]
        struct R<'a> {
            raw_url: &'a str,
            space_id: Option<i64>,
            title: Option<&'a str>,
            source: Option<&'a str>,
        }
        call(
            &self.base_url,
            "archive_url",
            &R {
                raw_url,
                space_id,
                title,
                source,
            },
        )
        .await
    }

    async fn auto_archive_from_text(&self, text: &str, space_id: i64) -> Result<usize> {
        #[derive(Serialize)]
        struct R<'a> {
            text: &'a str,
            space_id: i64,
        }
        call(
            &self.base_url,
            "auto_archive_from_text",
            &R { text, space_id },
        )
        .await
    }

    async fn check_urls_status(&self, urls: &[String]) -> Result<HashMap<String, String>> {
        #[derive(Serialize)]
        struct R<'a> {
            urls: &'a [String],
        }
        call(&self.base_url, "check_urls_status", &R { urls }).await
    }

    async fn trash_page(&self, page_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            page_id: i64,
        }
        call(&self.base_url, "trash_page", &R { page_id }).await
    }

    async fn restore_page(&self, page_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            page_id: i64,
        }
        call(&self.base_url, "restore_page", &R { page_id }).await
    }

    async fn delete_page_permanent(&self, page_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            page_id: i64,
        }
        call(&self.base_url, "delete_page_permanent", &R { page_id }).await
    }

    async fn update_page_status(&self, page_id: i64, status: &str) -> Result<()> {
        #[derive(Serialize)]
        struct R<'a> {
            page_id: i64,
            status: &'a str,
        }
        call(&self.base_url, "update_page_status", &R { page_id, status }).await
    }

    // ---- Files ----

    async fn list_files(&self, space_id: i64) -> Result<Vec<FileRow>> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "list_files", &R { space_id }).await
    }

    async fn get_file(&self, file_id: i64) -> Result<FileRow> {
        #[derive(Serialize)]
        struct R {
            file_id: i64,
        }
        call(&self.base_url, "get_file", &R { file_id }).await
    }

    async fn get_file_data(&self, file_id: i64) -> Result<(Option<String>, Vec<u8>)> {
        #[derive(Serialize)]
        struct R {
            file_id: i64,
        }
        let dto: FileBytesDto = call(&self.base_url, "get_file_data", &R { file_id }).await?;
        Ok((dto.mime_type, decode_b64(&dto.data_b64)?))
    }

    async fn insert_file(
        &self,
        name: &str,
        mime_type: Option<&str>,
        data: &[u8],
        space_id: i64,
    ) -> Result<(i64, InsertFileOutcome)> {
        #[derive(Serialize)]
        struct R<'a> {
            name: &'a str,
            mime_type: Option<&'a str>,
            data_b64: String,
            space_id: i64,
        }
        call(
            &self.base_url,
            "insert_file",
            &R {
                name,
                mime_type,
                data_b64: encode_b64(data),
                space_id,
            },
        )
        .await
    }

    async fn trash_file(&self, file_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            file_id: i64,
        }
        call(&self.base_url, "trash_file", &R { file_id }).await
    }

    async fn restore_file(&self, file_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            file_id: i64,
        }
        call(&self.base_url, "restore_file", &R { file_id }).await
    }

    async fn delete_file_permanent(&self, file_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            file_id: i64,
        }
        call(&self.base_url, "delete_file_permanent", &R { file_id }).await
    }

    // ---- Attachments ----

    async fn list_attachments(&self, note_id: i64) -> Result<Vec<AttachmentRow>> {
        #[derive(Serialize)]
        struct R {
            note_id: i64,
        }
        call(&self.base_url, "list_attachments", &R { note_id }).await
    }

    async fn list_removed_attachments(&self, note_id: i64) -> Result<Vec<AttachmentRow>> {
        #[derive(Serialize)]
        struct R {
            note_id: i64,
        }
        call(&self.base_url, "list_removed_attachments", &R { note_id }).await
    }

    async fn get_attachment(&self, attachment_id: i64) -> Result<AttachmentRow> {
        #[derive(Serialize)]
        struct R {
            attachment_id: i64,
        }
        call(&self.base_url, "get_attachment", &R { attachment_id }).await
    }

    async fn get_attachment_data(&self, attachment_id: i64) -> Result<(String, Vec<u8>)> {
        #[derive(Serialize)]
        struct R {
            attachment_id: i64,
        }
        let dto: AttachmentBytesDto =
            call(&self.base_url, "get_attachment_data", &R { attachment_id }).await?;
        Ok((dto.mime_type, decode_b64(&dto.data_b64)?))
    }

    async fn insert_attachment(
        &self,
        note_id: i64,
        name: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<(i64, InsertAttachmentOutcome)> {
        #[derive(Serialize)]
        struct R<'a> {
            note_id: i64,
            name: &'a str,
            mime_type: &'a str,
            data_b64: String,
        }
        call(
            &self.base_url,
            "insert_attachment",
            &R {
                note_id,
                name,
                mime_type,
                data_b64: encode_b64(data),
            },
        )
        .await
    }

    async fn cleanup_orphaned_attachments(&self, note_id: i64, used_ids: &[i64]) -> Result<usize> {
        #[derive(Serialize)]
        struct R<'a> {
            note_id: i64,
            used_ids: &'a [i64],
        }
        call(
            &self.base_url,
            "cleanup_orphaned_attachments",
            &R { note_id, used_ids },
        )
        .await
    }

    async fn restore_attachment(&self, attachment_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct R {
            attachment_id: i64,
        }
        call(&self.base_url, "restore_attachment", &R { attachment_id }).await
    }

    // ---- Trash ----

    async fn list_trash(&self, space_id: i64) -> Result<Vec<TrashItem>> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "list_trash", &R { space_id }).await
    }

    async fn trash_count(&self, space_id: i64) -> Result<i64> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
        }
        call(&self.base_url, "trash_count", &R { space_id }).await
    }

    // ---- Activity ----

    async fn activity_by_day(&self, space_id: i64, days: i64) -> Result<Vec<(String, i64)>> {
        #[derive(Serialize)]
        struct R {
            space_id: i64,
            days: i64,
        }
        call(&self.base_url, "activity_by_day", &R { space_id, days }).await
    }

    async fn activity_for_day(
        &self,
        space_id: i64,
        day: &str,
    ) -> Result<(Vec<NoteRow>, Vec<PageRef>)> {
        #[derive(Serialize)]
        struct R<'a> {
            space_id: i64,
            day: &'a str,
        }
        call(&self.base_url, "activity_for_day", &R { space_id, day }).await
    }

    // ---- Classification rules ----

    async fn load_rules(&self) -> Result<Vec<ClassificationRule>> {
        call(&self.base_url, "load_rules", &EmptyReq).await
    }

    // ---- FTS5 search ----

    async fn search_pages_brief(
        &self,
        query: &str,
        space_id: i64,
        limit: usize,
    ) -> Result<Vec<WebPageRow>> {
        #[derive(Serialize)]
        struct R<'a> {
            query: &'a str,
            space_id: i64,
            limit: usize,
        }
        call(
            &self.base_url,
            "search_pages_brief",
            &R {
                query,
                space_id,
                limit,
            },
        )
        .await
    }

    async fn search_notes(&self, query: &str, space_id: i64, limit: usize) -> Result<Vec<NoteRow>> {
        #[derive(Serialize)]
        struct R<'a> {
            query: &'a str,
            space_id: i64,
            limit: usize,
        }
        call(
            &self.base_url,
            "search_notes",
            &R {
                query,
                space_id,
                limit,
            },
        )
        .await
    }
}
