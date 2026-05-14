//! Backend trait — abstracts data access for desktop vs (future) web.
//!
//! The trait is `async` because the web variant will speak HTTP; the desktop
//! variant `LocalBackend` wraps synchronous `lore_core::db::*` calls in
//! `async {}` blocks (futures resolve immediately, no real blocking).
//!
//! `DataStore` (and views) interact with `Arc<dyn Backend>` so the platform
//! swap is a single field. The trait method signatures are 1:1 with the
//! existing DB call sites in `store.rs` / `main.rs`; no domain model is
//! introduced — that's deliberate, see CLAUDE.md (web-version phasing).

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use lore_core::db::{
    ArchiveOutcome, AttachmentRow, ClassificationRule, FileRow, FolderRow, InsertAttachmentOutcome,
    InsertFileOutcome, NoteData, NoteRow, PageRef, SpaceRow, SpaceStats, TrashItem, WebPageDetail,
    WebPageRow,
};

pub mod local;

pub use local::LocalBackend;

/// Process-wide backend handle. Set once at startup (`init`) and read by
/// every `DataStore` mutation. Living as a global keeps `DataStore` `Copy`
/// — Dioxus signals work much better with Copy state — and avoids passing
/// `Arc<dyn Backend>` through every component boundary.
static BACKEND: OnceLock<Arc<dyn Backend>> = OnceLock::new();

/// Install the backend. Call exactly once at startup. Panics if called twice
/// — that would indicate the boot path ran more than once.
pub fn init(backend: Arc<dyn Backend>) {
    BACKEND
        .set(backend)
        .map_err(|_| ())
        .expect("backend::init called more than once");
}

/// Get a handle to the active backend. Panics if `init` hasn't been called yet,
/// which would mean something tried to use the data layer before boot.
pub fn current() -> Arc<dyn Backend> {
    BACKEND
        .get()
        .expect("backend::current called before init")
        .clone()
}

/// Async data-access surface used by `DataStore` and a few UI helpers
/// (keyboard nav, file save dialog). Each method maps 1:1 to one DB call
/// today; future `HttpBackend` will map each to one server endpoint.
///
/// `Send + Sync` is required so `Arc<dyn Backend>` works across spawned tasks.
#[async_trait]
pub trait Backend: Send + Sync {
    // ---- Bootstrap ----

    async fn get_revision(&self) -> Result<i64>;

    /// Raw `PRAGMA user_version` read. Bypasses the bootstrap "refuse newer
    /// schema" gate so the polling loop can detect a migration happening
    /// under us without crashing.
    async fn db_schema_version(&self) -> Result<u32>;

    // ---- Spaces ----

    async fn list_spaces(&self) -> Result<Vec<SpaceRow>>;
    async fn list_all_spaces(&self) -> Result<Vec<SpaceRow>>;
    /// Most recently used non-deleted space — the one we auto-select at boot
    /// and what `SpaceRenameInput` falls back to when the user deletes the
    /// active space inline.
    async fn get_active_space(&self) -> Result<SpaceRow>;
    /// Counts + total byte sizes for the Settings/Spaces detail view.
    async fn space_stats(&self, space_id: i64) -> Result<SpaceStats>;
    async fn touch_space(&self, space_id: i64) -> Result<()>;
    async fn create_space(&self, name: &str) -> Result<i64>;
    async fn rename_space(&self, space_id: i64, name: &str) -> Result<()>;
    async fn trash_space(&self, space_id: i64) -> Result<()>;
    async fn restore_space(&self, space_id: i64) -> Result<()>;
    async fn delete_space_permanent(&self, space_id: i64) -> Result<()>;

    // ---- Folders ----

    async fn list_folders(&self, space_id: i64) -> Result<Vec<FolderRow>>;
    async fn folder_note_counts(&self, space_id: i64) -> Result<HashMap<i64, i64>>;
    async fn create_folder(&self, name: &str, parent_id: Option<i64>, space_id: i64)
    -> Result<i64>;
    async fn rename_folder(&self, folder_id: i64, name: &str) -> Result<()>;
    async fn delete_folder(&self, folder_id: i64) -> Result<()>;

    // ---- Notes ----

    async fn list_notes(&self, folder_id: Option<i64>, space_id: i64) -> Result<Vec<NoteRow>>;
    async fn list_note_ids_ordered(
        &self,
        folder_id: Option<i64>,
        space_id: i64,
    ) -> Result<Vec<i64>>;
    async fn get_note(&self, note_id: i64) -> Result<NoteData>;
    async fn create_note(
        &self,
        title: &str,
        body: &str,
        folder_id: Option<i64>,
        space_id: i64,
    ) -> Result<i64>;
    async fn update_note(&self, note_id: i64, title: &str, body: &str) -> Result<()>;
    async fn move_note(&self, note_id: i64, folder_id: Option<i64>) -> Result<()>;
    async fn trash_note(&self, note_id: i64) -> Result<()>;
    async fn restore_note(&self, note_id: i64) -> Result<()>;
    async fn delete_note_permanent(&self, note_id: i64) -> Result<()>;
    async fn find_notes_referencing_url(
        &self,
        url: &str,
        space_id: i64,
    ) -> Result<Vec<(i64, String)>>;

    // ---- Pages ----

    async fn list_pages(&self, space_id: i64, limit: usize) -> Result<Vec<WebPageRow>>;
    async fn list_page_ids_ordered(&self, space_id: i64, limit: usize) -> Result<Vec<i64>>;
    async fn get_page(&self, page_id: i64) -> Result<WebPageDetail>;
    async fn archive_url(
        &self,
        raw_url: &str,
        space_id: Option<i64>,
        title: Option<&str>,
        source: Option<&str>,
    ) -> Result<ArchiveOutcome>;
    async fn auto_archive_from_text(&self, text: &str, space_id: i64) -> Result<usize>;
    async fn check_urls_status(&self, urls: &[String]) -> Result<HashMap<String, String>>;
    async fn trash_page(&self, page_id: i64) -> Result<()>;
    async fn restore_page(&self, page_id: i64) -> Result<()>;
    async fn delete_page_permanent(&self, page_id: i64) -> Result<()>;
    async fn update_page_status(&self, page_id: i64, status: &str) -> Result<()>;

    // ---- Files ----

    async fn list_files(&self, space_id: i64) -> Result<Vec<FileRow>>;
    async fn get_file(&self, file_id: i64) -> Result<FileRow>;
    async fn get_file_data(&self, file_id: i64) -> Result<(Option<String>, Vec<u8>)>;
    async fn insert_file(
        &self,
        name: &str,
        mime_type: Option<&str>,
        data: &[u8],
        space_id: i64,
    ) -> Result<(i64, InsertFileOutcome)>;
    async fn trash_file(&self, file_id: i64) -> Result<()>;
    async fn restore_file(&self, file_id: i64) -> Result<()>;
    async fn delete_file_permanent(&self, file_id: i64) -> Result<()>;

    // ---- Attachments ----

    async fn list_attachments(&self, note_id: i64) -> Result<Vec<AttachmentRow>>;
    async fn list_removed_attachments(&self, note_id: i64) -> Result<Vec<AttachmentRow>>;
    async fn get_attachment(&self, attachment_id: i64) -> Result<AttachmentRow>;
    async fn get_attachment_data(&self, attachment_id: i64) -> Result<(String, Vec<u8>)>;
    async fn insert_attachment(
        &self,
        note_id: i64,
        name: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<(i64, InsertAttachmentOutcome)>;
    async fn cleanup_orphaned_attachments(&self, note_id: i64, used_ids: &[i64]) -> Result<usize>;
    async fn restore_attachment(&self, attachment_id: i64) -> Result<()>;

    // ---- Trash ----

    async fn list_trash(&self, space_id: i64) -> Result<Vec<TrashItem>>;
    async fn trash_count(&self, space_id: i64) -> Result<i64>;

    // ---- Activity ----

    async fn activity_by_day(&self, space_id: i64, days: i64) -> Result<Vec<(String, i64)>>;
    async fn activity_for_day(
        &self,
        space_id: i64,
        day: &str,
    ) -> Result<(Vec<NoteRow>, Vec<PageRef>)>;

    // ---- Classification rules (read-only — rules are seeded, not user-edited) ----

    async fn load_rules(&self) -> Result<Vec<ClassificationRule>>;

    // ---- FTS5 search ----

    async fn search_pages_brief(
        &self,
        query: &str,
        space_id: i64,
        limit: usize,
    ) -> Result<Vec<WebPageRow>>;
    async fn search_notes(&self, query: &str, space_id: i64, limit: usize) -> Result<Vec<NoteRow>>;
}
