//! `LocalBackend` — in-process implementation of `Backend` that opens
//! a fresh SQLite connection per call (same model as today's `data::open_db()`
//! pattern; per-call open is cheap with the connection-cache PRAGMAs).
//!
//! Every method wraps a synchronous `lore_core::db::*` call in an `async`
//! block. The future resolves immediately — there is no real awaiting, this
//! just satisfies the async trait so the same call sites work against a
//! future `HttpBackend`.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;

use lore_core::db::{
    self, ArchiveOutcome, AttachmentRow, ClassificationRule, FileRow, FolderRow,
    InsertAttachmentOutcome, InsertFileOutcome, NoteData, NoteRow, PageRef, SpaceRow, SpaceStats,
    TrashItem, WebPageDetail, WebPageRow,
};
use lore_core::search;

use super::{Backend, Result};

/// `anyhow::Result<T>` → `Result<T, BackendError>`. The `From<anyhow::Error>`
/// impl in `lore_core::error` knows to map `rusqlite::Error::QueryReturnedNoRows`
/// to `ErrorCode::NotFound`, so a per-handler `not_found` vs `internal`
/// classification falls out for free here.
fn ok<T>(r: anyhow::Result<T>) -> Result<T> {
    r.map_err(Into::into)
}

#[derive(Clone)]
pub struct LocalBackend {
    db_path: PathBuf,
}

impl LocalBackend {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn conn(&self) -> Result<rusqlite::Connection> {
        ok(db::open_existing(&self.db_path))
    }
}

#[async_trait(?Send)]
impl Backend for LocalBackend {
    // ---- Bootstrap ----

    async fn get_revision(&self) -> Result<i64> {
        ok(db::get_revision(&self.conn()?))
    }

    async fn db_schema_version(&self) -> Result<u32> {
        // Raw open — no `open_existing` pragmas — so an under-us migration
        // bump doesn't trip the WAL/foreign-key apply step.
        let conn = rusqlite::Connection::open(&self.db_path)?;
        let v: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        Ok(v)
    }

    // ---- Spaces ----

    async fn list_spaces(&self) -> Result<Vec<SpaceRow>> {
        ok(db::list_spaces(&self.conn()?))
    }

    async fn list_all_spaces(&self) -> Result<Vec<SpaceRow>> {
        ok(db::list_all_spaces(&self.conn()?))
    }

    async fn get_active_space(&self) -> Result<SpaceRow> {
        ok(db::get_active_space(&self.conn()?))
    }

    async fn space_stats(&self, space_id: i64) -> Result<SpaceStats> {
        ok(db::space_stats(&self.conn()?, space_id))
    }

    async fn touch_space(&self, space_id: i64) -> Result<()> {
        ok(db::touch_space(&self.conn()?, space_id))
    }

    async fn create_space(&self, name: &str) -> Result<i64> {
        ok(db::insert_space(&self.conn()?, name))
    }

    async fn rename_space(&self, space_id: i64, name: &str) -> Result<()> {
        ok(db::rename_space(&self.conn()?, space_id, name))
    }

    async fn trash_space(&self, space_id: i64) -> Result<()> {
        ok(db::trash_space(&self.conn()?, space_id))
    }

    async fn restore_space(&self, space_id: i64) -> Result<()> {
        ok(db::restore_space(&self.conn()?, space_id))
    }

    async fn delete_space_permanent(&self, space_id: i64) -> Result<()> {
        ok(db::delete_space_permanent(&self.conn()?, space_id))
    }

    // ---- Folders ----

    async fn list_folders(&self, space_id: i64) -> Result<Vec<FolderRow>> {
        ok(db::list_folders(&self.conn()?, space_id))
    }

    async fn folder_note_counts(&self, space_id: i64) -> Result<HashMap<i64, i64>> {
        ok(db::folder_note_counts(&self.conn()?, space_id))
    }

    async fn create_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        space_id: i64,
    ) -> Result<i64> {
        ok(db::insert_folder(&self.conn()?, name, parent_id, space_id))
    }

    async fn rename_folder(&self, folder_id: i64, name: &str) -> Result<()> {
        ok(db::rename_folder(&self.conn()?, folder_id, name))
    }

    async fn delete_folder(&self, folder_id: i64) -> Result<()> {
        ok(db::delete_folder(&self.conn()?, folder_id))
    }

    // ---- Notes ----

    async fn list_notes(&self, folder_id: Option<i64>, space_id: i64) -> Result<Vec<NoteRow>> {
        ok(db::list_notes(&self.conn()?, folder_id, space_id))
    }

    async fn list_note_ids_ordered(
        &self,
        folder_id: Option<i64>,
        space_id: i64,
    ) -> Result<Vec<i64>> {
        ok(db::list_note_ids_ordered(
            &self.conn()?,
            folder_id,
            space_id,
        ))
    }

    async fn get_note(&self, note_id: i64) -> Result<NoteData> {
        ok(db::get_note(&self.conn()?, note_id))
    }

    async fn create_note(
        &self,
        title: &str,
        body: &str,
        folder_id: Option<i64>,
        space_id: i64,
    ) -> Result<i64> {
        ok(db::insert_note(
            &self.conn()?,
            title,
            body,
            folder_id,
            space_id,
        ))
    }

    async fn update_note(&self, note_id: i64, title: &str, body: &str) -> Result<()> {
        ok(db::update_note(&self.conn()?, note_id, title, body))
    }

    async fn move_note(&self, note_id: i64, folder_id: Option<i64>) -> Result<()> {
        ok(db::move_note_to_folder(&self.conn()?, note_id, folder_id))
    }

    async fn trash_note(&self, note_id: i64) -> Result<()> {
        ok(db::trash_note(&self.conn()?, note_id))
    }

    async fn restore_note(&self, note_id: i64) -> Result<()> {
        ok(db::restore_note_safe(&self.conn()?, note_id))
    }

    async fn delete_note_permanent(&self, note_id: i64) -> Result<()> {
        ok(db::delete_note_permanent(&self.conn()?, note_id))
    }

    async fn find_notes_referencing_url(
        &self,
        url: &str,
        space_id: i64,
    ) -> Result<Vec<(i64, String)>> {
        ok(db::find_notes_referencing_url(&self.conn()?, url, space_id))
    }

    // ---- Pages ----

    async fn list_pages(&self, space_id: i64, limit: usize) -> Result<Vec<WebPageRow>> {
        ok(db::list_pages(&self.conn()?, space_id, limit))
    }

    async fn list_page_ids_ordered(&self, space_id: i64, limit: usize) -> Result<Vec<i64>> {
        ok(db::list_page_ids_ordered(&self.conn()?, space_id, limit))
    }

    async fn get_page(&self, page_id: i64) -> Result<WebPageDetail> {
        ok(db::get_page(&self.conn()?, page_id))
    }

    async fn archive_url(
        &self,
        raw_url: &str,
        space_id: Option<i64>,
        title: Option<&str>,
        source: Option<&str>,
    ) -> Result<ArchiveOutcome> {
        ok(db::archive_url(
            &self.conn()?,
            raw_url,
            space_id,
            title,
            source,
        ))
    }

    async fn auto_archive_from_text(&self, text: &str, space_id: i64) -> Result<usize> {
        ok(db::auto_archive_from_text(&self.conn()?, text, space_id))
    }

    async fn check_urls_status(&self, urls: &[String]) -> Result<HashMap<String, String>> {
        ok(db::check_urls_status(&self.conn()?, urls))
    }

    async fn trash_page(&self, page_id: i64) -> Result<()> {
        ok(db::trash_page(&self.conn()?, page_id))
    }

    async fn restore_page(&self, page_id: i64) -> Result<()> {
        ok(db::restore_page(&self.conn()?, page_id))
    }

    async fn delete_page_permanent(&self, page_id: i64) -> Result<()> {
        ok(db::delete_page(&self.conn()?, page_id))
    }

    async fn update_page_status(&self, page_id: i64, status: &str) -> Result<()> {
        ok(db::update_status(&self.conn()?, page_id, status))
    }

    // ---- Files ----

    async fn list_files(&self, space_id: i64) -> Result<Vec<FileRow>> {
        ok(db::list_files(&self.conn()?, space_id))
    }

    async fn get_file(&self, file_id: i64) -> Result<FileRow> {
        ok(db::get_file(&self.conn()?, file_id))
    }

    async fn get_file_data(&self, file_id: i64) -> Result<(Option<String>, Vec<u8>)> {
        ok(db::get_file_data(&self.conn()?, file_id))
    }

    async fn insert_file(
        &self,
        name: &str,
        mime_type: Option<&str>,
        data: &[u8],
        space_id: i64,
    ) -> Result<(i64, InsertFileOutcome)> {
        ok(db::insert_file(
            &self.conn()?,
            name,
            mime_type,
            data,
            space_id,
        ))
    }

    async fn trash_file(&self, file_id: i64) -> Result<()> {
        ok(db::trash_file(&self.conn()?, file_id))
    }

    async fn restore_file(&self, file_id: i64) -> Result<()> {
        ok(db::restore_file(&self.conn()?, file_id))
    }

    async fn delete_file_permanent(&self, file_id: i64) -> Result<()> {
        ok(db::delete_file_permanent(&self.conn()?, file_id))
    }

    // ---- Attachments ----

    async fn list_attachments(&self, note_id: i64) -> Result<Vec<AttachmentRow>> {
        ok(db::list_attachments(&self.conn()?, note_id))
    }

    async fn list_removed_attachments(&self, note_id: i64) -> Result<Vec<AttachmentRow>> {
        ok(db::list_removed_attachments(&self.conn()?, note_id))
    }

    async fn get_attachment(&self, attachment_id: i64) -> Result<AttachmentRow> {
        ok(db::get_attachment(&self.conn()?, attachment_id))
    }

    async fn get_attachment_data(&self, attachment_id: i64) -> Result<(String, Vec<u8>)> {
        ok(db::get_attachment_data(&self.conn()?, attachment_id))
    }

    async fn insert_attachment(
        &self,
        note_id: i64,
        name: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<(i64, InsertAttachmentOutcome)> {
        ok(db::insert_attachment(
            &self.conn()?,
            note_id,
            name,
            mime_type,
            data,
        ))
    }

    async fn cleanup_orphaned_attachments(&self, note_id: i64, used_ids: &[i64]) -> Result<usize> {
        ok(db::cleanup_orphaned_attachments(
            &self.conn()?,
            note_id,
            used_ids,
        ))
    }

    async fn restore_attachment(&self, attachment_id: i64) -> Result<()> {
        ok(db::restore_attachment(&self.conn()?, attachment_id))
    }

    // ---- Trash ----

    async fn list_trash(&self, space_id: i64) -> Result<Vec<TrashItem>> {
        ok(db::list_trash(&self.conn()?, space_id))
    }

    async fn trash_count(&self, space_id: i64) -> Result<i64> {
        ok(db::trash_count(&self.conn()?, space_id))
    }

    // ---- Activity ----

    async fn activity_by_day(&self, space_id: i64, days: i64) -> Result<Vec<(String, i64)>> {
        ok(db::activity_by_day(&self.conn()?, space_id, days))
    }

    async fn activity_for_day(
        &self,
        space_id: i64,
        day: &str,
    ) -> Result<(Vec<NoteRow>, Vec<PageRef>)> {
        ok(db::activity_for_day(&self.conn()?, space_id, day))
    }

    // ---- Classification rules ----

    async fn load_rules(&self) -> Result<Vec<ClassificationRule>> {
        ok(db::load_rules(&self.conn()?))
    }

    // ---- FTS5 search ----

    async fn search_pages_brief(
        &self,
        query: &str,
        space_id: i64,
        limit: usize,
    ) -> Result<Vec<WebPageRow>> {
        ok(search::search_web_pages_brief(
            &self.conn()?,
            query,
            space_id,
            limit,
        ))
    }

    async fn search_notes(&self, query: &str, space_id: i64, limit: usize) -> Result<Vec<NoteRow>> {
        ok(search::search_notes(&self.conn()?, query, space_id, limit))
    }
}
