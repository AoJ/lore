//! `HttpBackend` — talks to `lore-server`'s JSON API. Compiled only on
//! the `web` feature so its `gloo-net` dependency (browser-only) doesn't
//! leak into desktop builds.
//!
//! W3b lands the stub: every method panics with `todo!`. W3c fills in
//! the actual `fetch` calls.

use async_trait::async_trait;
use std::collections::HashMap;

use lore_core::db::{
    ArchiveOutcome, AttachmentRow, ClassificationRule, FileRow, FolderRow, InsertAttachmentOutcome,
    InsertFileOutcome, NoteData, NoteRow, PageRef, SpaceRow, SpaceStats, TrashItem, WebPageDetail,
    WebPageRow,
};

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

#[async_trait]
impl Backend for HttpBackend {
    // ---- Bootstrap ----

    async fn get_revision(&self) -> Result<i64> {
        todo!("W3c: POST /api/get_revision")
    }

    async fn db_schema_version(&self) -> Result<u32> {
        todo!("W3c: POST /api/db_schema_version")
    }

    // ---- Spaces ----

    async fn list_spaces(&self) -> Result<Vec<SpaceRow>> {
        todo!("W3c: POST /api/list_spaces")
    }

    async fn list_all_spaces(&self) -> Result<Vec<SpaceRow>> {
        todo!("W3c: POST /api/list_all_spaces")
    }

    async fn get_active_space(&self) -> Result<SpaceRow> {
        todo!("W3c: POST /api/get_active_space")
    }

    async fn space_stats(&self, _space_id: i64) -> Result<SpaceStats> {
        todo!("W3c: POST /api/space_stats")
    }

    async fn touch_space(&self, _space_id: i64) -> Result<()> {
        todo!("W3c: POST /api/touch_space")
    }

    async fn create_space(&self, _name: &str) -> Result<i64> {
        todo!("W3c: POST /api/create_space")
    }

    async fn rename_space(&self, _space_id: i64, _name: &str) -> Result<()> {
        todo!("W3c: POST /api/rename_space")
    }

    async fn trash_space(&self, _space_id: i64) -> Result<()> {
        todo!("W3c: POST /api/trash_space")
    }

    async fn restore_space(&self, _space_id: i64) -> Result<()> {
        todo!("W3c: POST /api/restore_space")
    }

    async fn delete_space_permanent(&self, _space_id: i64) -> Result<()> {
        todo!("W3c: POST /api/delete_space_permanent")
    }

    // ---- Folders ----

    async fn list_folders(&self, _space_id: i64) -> Result<Vec<FolderRow>> {
        todo!("W3c: POST /api/list_folders")
    }

    async fn folder_note_counts(&self, _space_id: i64) -> Result<HashMap<i64, i64>> {
        todo!("W3c: POST /api/folder_note_counts")
    }

    async fn create_folder(
        &self,
        _name: &str,
        _parent_id: Option<i64>,
        _space_id: i64,
    ) -> Result<i64> {
        todo!("W3c: POST /api/create_folder")
    }

    async fn rename_folder(&self, _folder_id: i64, _name: &str) -> Result<()> {
        todo!("W3c: POST /api/rename_folder")
    }

    async fn delete_folder(&self, _folder_id: i64) -> Result<()> {
        todo!("W3c: POST /api/delete_folder")
    }

    // ---- Notes ----

    async fn list_notes(&self, _folder_id: Option<i64>, _space_id: i64) -> Result<Vec<NoteRow>> {
        todo!("W3c: POST /api/list_notes")
    }

    async fn list_note_ids_ordered(
        &self,
        _folder_id: Option<i64>,
        _space_id: i64,
    ) -> Result<Vec<i64>> {
        todo!("W3c: POST /api/list_note_ids_ordered")
    }

    async fn get_note(&self, _note_id: i64) -> Result<NoteData> {
        todo!("W3c: POST /api/get_note")
    }

    async fn create_note(
        &self,
        _title: &str,
        _body: &str,
        _folder_id: Option<i64>,
        _space_id: i64,
    ) -> Result<i64> {
        todo!("W3c: POST /api/create_note")
    }

    async fn update_note(&self, _note_id: i64, _title: &str, _body: &str) -> Result<()> {
        todo!("W3c: POST /api/update_note")
    }

    async fn move_note(&self, _note_id: i64, _folder_id: Option<i64>) -> Result<()> {
        todo!("W3c: POST /api/move_note")
    }

    async fn trash_note(&self, _note_id: i64) -> Result<()> {
        todo!("W3c: POST /api/trash_note")
    }

    async fn restore_note(&self, _note_id: i64) -> Result<()> {
        todo!("W3c: POST /api/restore_note")
    }

    async fn delete_note_permanent(&self, _note_id: i64) -> Result<()> {
        todo!("W3c: POST /api/delete_note_permanent")
    }

    async fn find_notes_referencing_url(
        &self,
        _url: &str,
        _space_id: i64,
    ) -> Result<Vec<(i64, String)>> {
        todo!("W3c: POST /api/find_notes_referencing_url")
    }

    // ---- Pages ----

    async fn list_pages(&self, _space_id: i64, _limit: usize) -> Result<Vec<WebPageRow>> {
        todo!("W3c: POST /api/list_pages")
    }

    async fn list_page_ids_ordered(&self, _space_id: i64, _limit: usize) -> Result<Vec<i64>> {
        todo!("W3c: POST /api/list_page_ids_ordered")
    }

    async fn get_page(&self, _page_id: i64) -> Result<WebPageDetail> {
        todo!("W3c: POST /api/get_page")
    }

    async fn archive_url(
        &self,
        _raw_url: &str,
        _space_id: Option<i64>,
        _title: Option<&str>,
        _source: Option<&str>,
    ) -> Result<ArchiveOutcome> {
        todo!("W3c: POST /api/archive_url")
    }

    async fn auto_archive_from_text(&self, _text: &str, _space_id: i64) -> Result<usize> {
        todo!("W3c: POST /api/auto_archive_from_text")
    }

    async fn check_urls_status(&self, _urls: &[String]) -> Result<HashMap<String, String>> {
        todo!("W3c: POST /api/check_urls_status")
    }

    async fn trash_page(&self, _page_id: i64) -> Result<()> {
        todo!("W3c: POST /api/trash_page")
    }

    async fn restore_page(&self, _page_id: i64) -> Result<()> {
        todo!("W3c: POST /api/restore_page")
    }

    async fn delete_page_permanent(&self, _page_id: i64) -> Result<()> {
        todo!("W3c: POST /api/delete_page_permanent")
    }

    async fn update_page_status(&self, _page_id: i64, _status: &str) -> Result<()> {
        todo!("W3c: POST /api/update_page_status")
    }

    // ---- Files ----

    async fn list_files(&self, _space_id: i64) -> Result<Vec<FileRow>> {
        todo!("W3c: POST /api/list_files")
    }

    async fn get_file(&self, _file_id: i64) -> Result<FileRow> {
        todo!("W3c: POST /api/get_file")
    }

    async fn get_file_data(&self, _file_id: i64) -> Result<(Option<String>, Vec<u8>)> {
        todo!("W3c: POST /api/get_file_data")
    }

    async fn insert_file(
        &self,
        _name: &str,
        _mime_type: Option<&str>,
        _data: &[u8],
        _space_id: i64,
    ) -> Result<(i64, InsertFileOutcome)> {
        todo!("W3c: POST /api/insert_file")
    }

    async fn trash_file(&self, _file_id: i64) -> Result<()> {
        todo!("W3c: POST /api/trash_file")
    }

    async fn restore_file(&self, _file_id: i64) -> Result<()> {
        todo!("W3c: POST /api/restore_file")
    }

    async fn delete_file_permanent(&self, _file_id: i64) -> Result<()> {
        todo!("W3c: POST /api/delete_file_permanent")
    }

    // ---- Attachments ----

    async fn list_attachments(&self, _note_id: i64) -> Result<Vec<AttachmentRow>> {
        todo!("W3c: POST /api/list_attachments")
    }

    async fn list_removed_attachments(&self, _note_id: i64) -> Result<Vec<AttachmentRow>> {
        todo!("W3c: POST /api/list_removed_attachments")
    }

    async fn get_attachment(&self, _attachment_id: i64) -> Result<AttachmentRow> {
        todo!("W3c: POST /api/get_attachment")
    }

    async fn get_attachment_data(&self, _attachment_id: i64) -> Result<(String, Vec<u8>)> {
        todo!("W3c: POST /api/get_attachment_data")
    }

    async fn insert_attachment(
        &self,
        _note_id: i64,
        _name: &str,
        _mime_type: &str,
        _data: &[u8],
    ) -> Result<(i64, InsertAttachmentOutcome)> {
        todo!("W3c: POST /api/insert_attachment")
    }

    async fn cleanup_orphaned_attachments(
        &self,
        _note_id: i64,
        _used_ids: &[i64],
    ) -> Result<usize> {
        todo!("W3c: POST /api/cleanup_orphaned_attachments")
    }

    async fn restore_attachment(&self, _attachment_id: i64) -> Result<()> {
        todo!("W3c: POST /api/restore_attachment")
    }

    // ---- Trash ----

    async fn list_trash(&self, _space_id: i64) -> Result<Vec<TrashItem>> {
        todo!("W3c: POST /api/list_trash")
    }

    async fn trash_count(&self, _space_id: i64) -> Result<i64> {
        todo!("W3c: POST /api/trash_count")
    }

    // ---- Activity ----

    async fn activity_by_day(&self, _space_id: i64, _days: i64) -> Result<Vec<(String, i64)>> {
        todo!("W3c: POST /api/activity_by_day")
    }

    async fn activity_for_day(
        &self,
        _space_id: i64,
        _day: &str,
    ) -> Result<(Vec<NoteRow>, Vec<PageRef>)> {
        todo!("W3c: POST /api/activity_for_day")
    }

    // ---- Classification rules ----

    async fn load_rules(&self) -> Result<Vec<ClassificationRule>> {
        todo!("W3c: POST /api/load_rules")
    }

    // ---- FTS5 search ----

    async fn search_pages_brief(
        &self,
        _query: &str,
        _space_id: i64,
        _limit: usize,
    ) -> Result<Vec<WebPageRow>> {
        todo!("W3c: POST /api/search_pages_brief")
    }

    async fn search_notes(
        &self,
        _query: &str,
        _space_id: i64,
        _limit: usize,
    ) -> Result<Vec<NoteRow>> {
        todo!("W3c: POST /api/search_notes")
    }
}
