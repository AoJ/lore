//! Centralized data store — single source of truth for all UI data.
//!
//! Views read signals from here, never call the backend directly. Mutations
//! go through store methods which update the backend then refresh signals.
//!
//! All mutation methods are `async`. Desktop's `LocalBackend` resolves them
//! immediately (sync work wrapped in async), but the same surface compiles
//! against `HttpBackend` for the future web target. Views call mutations
//! from event handlers via `spawn(async move { store.x().await })`.

use crate::backend;
use crate::state::{AppState, Section, Selected};
use dioxus::prelude::*;
use lore_core::db::{TrashItem, WebPageRow};
use std::collections::HashMap;

/// Central data store, provided as Dioxus context alongside AppState.
#[derive(Clone, Copy)]
pub struct DataStore {
    // ---- Cached data (read by components) ----
    pub pages: Signal<Vec<WebPageRow>>,
    pub notes: Signal<Vec<lore_core::db::NoteRow>>,
    pub files: Signal<Vec<lore_core::db::FileRow>>,
    pub folders: Signal<Vec<lore_core::db::FolderRow>>,
    pub spaces: Signal<Vec<lore_core::db::SpaceRow>>,
    pub trash_items: Signal<Vec<TrashItem>>,
    pub trash_count: Signal<i64>,
    pub note_counts: Signal<HashMap<i64, i64>>,
    pub revision: Signal<i64>,
    /// True when the on-disk DB was migrated to a newer version while this
    /// app instance is still running on the old schema. Polled separately
    /// from data revision; clearing requires app restart.
    pub schema_outdated: Signal<bool>,
    pub heatmap: Signal<Vec<(String, i64)>>, // (YYYY-MM-DD, count)
    pub timeline_selected_day: Signal<Option<String>>,
    pub timeline_day_notes: Signal<Vec<lore_core::db::NoteRow>>,
    pub timeline_day_pages: Signal<Vec<(i64, String)>>,

    // ---- URL indicators ----
    /// URLs from the currently open note — set by editor on each save
    pub current_note_urls: Signal<Vec<String>>,
    /// Cached status for those URLs — refreshed by polling
    pub url_statuses: Signal<HashMap<String, String>>,

    // ---- Internal ----
    last_poll_rev: Signal<i64>,
}

impl DataStore {
    pub fn new() -> Self {
        Self {
            pages: Signal::new(Vec::new()),
            notes: Signal::new(Vec::new()),
            files: Signal::new(Vec::new()),
            folders: Signal::new(Vec::new()),
            spaces: Signal::new(Vec::new()),
            trash_items: Signal::new(Vec::new()),
            trash_count: Signal::new(0),
            note_counts: Signal::new(HashMap::new()),
            revision: Signal::new(0),
            schema_outdated: Signal::new(false),
            heatmap: Signal::new(Vec::new()),
            timeline_selected_day: Signal::new(None),
            timeline_day_notes: Signal::new(Vec::new()),
            timeline_day_pages: Signal::new(Vec::new()),
            current_note_urls: Signal::new(Vec::new()),
            url_statuses: Signal::new(HashMap::new()),
            last_poll_rev: Signal::new(0),
        }
    }

    /// Called from polling loop — checks DB revision and schema version.
    pub async fn poll(&mut self, state: &AppState) {
        // Schema version check first: if the DB was migrated by another
        // process (CLI `lore migrate`, a newer build, …) we set a flag for
        // the UI banner. We don't try to recover — the live connections were
        // opened against the old schema and queries would start failing on
        // any new column. User has to restart.
        let on_disk = backend::current().db_schema_version().await.unwrap_or(0);
        let known = lore_core::migrations::EXPECTED_VERSION;
        if on_disk != known && !*self.schema_outdated.read() {
            self.schema_outdated.set(true);
        }

        let new_rev = backend::current().get_revision().await.unwrap_or(0);
        if new_rev != *self.last_poll_rev.read() {
            self.last_poll_rev.set(new_rev);
            self.revision.set(new_rev);
            self.refresh(state).await;
        }
    }

    /// Refresh all data for current view. Called after mutations and on poll.
    pub async fn refresh(&mut self, state: &AppState) {
        let space_id = *state.space_id.read();
        let section = state.section.read().clone();
        let b = backend::current();

        // Always refresh: spaces, folders, counts, trash
        self.spaces.set(b.list_spaces().await.unwrap_or_default());
        self.folders
            .set(b.list_folders(space_id).await.unwrap_or_default());
        self.note_counts
            .set(b.folder_note_counts(space_id).await.unwrap_or_default());
        self.trash_count
            .set(b.trash_count(space_id).await.unwrap_or(0));

        // Heatmap
        if matches!(section, Section::Timeline) {
            self.heatmap
                .set(b.activity_by_day(space_id, 30).await.unwrap_or_default());
        }

        // Refresh list for current section
        match section {
            Section::AllPages => {
                self.pages
                    .set(b.list_pages(space_id, 200).await.unwrap_or_default());
            }
            Section::AllNotes => {
                self.notes
                    .set(b.list_notes(None, space_id).await.unwrap_or_default());
            }
            Section::Folder(folder_id) => {
                self.notes.set(
                    b.list_notes(Some(folder_id), space_id)
                        .await
                        .unwrap_or_default(),
                );
            }
            Section::AllFiles => {
                self.files
                    .set(b.list_files(space_id).await.unwrap_or_default());
            }
            Section::Trash => {
                self.trash_items
                    .set(b.list_trash(space_id).await.unwrap_or_default());
            }
            _ => {}
        }

        // Refresh URL statuses for current note
        let urls = self.current_note_urls.read().clone();
        if !urls.is_empty()
            && let Ok(statuses) = b.check_urls_status(&urls).await
        {
            self.url_statuses.set(statuses);
        }

        self.revision.set(b.get_revision().await.unwrap_or(0));
    }

    // ---- Timeline ----

    pub async fn select_timeline_day(&mut self, state: &AppState, day: &str) {
        let space_id = *state.space_id.read();
        self.timeline_selected_day.set(Some(day.to_string()));
        if let Ok((notes, pages)) = backend::current().activity_for_day(space_id, day).await {
            self.timeline_day_notes.set(notes);
            self.timeline_day_pages.set(pages);
        }
    }

    // ---- Navigation (immediate refresh on section/space change) ----

    pub async fn navigate(&mut self, state: &mut AppState, section: Section) {
        state.section.set(section);
        state.selected.set(Selected::None);
        self.refresh(state).await;
    }

    pub async fn switch_space(&mut self, state: &mut AppState, space_id: i64) {
        state.space_id.set(space_id);
        state.section.set(Section::AllNotes);
        state.selected.set(Selected::None);
        state.space_dropdown_open.set(false);
        backend::current().touch_space(space_id).await.ok();
        self.refresh(state).await;
    }

    // ---- Note mutations ----

    pub async fn save_note(&mut self, note_id: i64, title: &str, body: &str) -> Result<(), String> {
        backend::current()
            .update_note(note_id, title, body)
            .await
            .map_err(|e| e.to_string())?;
        // Don't refresh list on every keystroke — polling will catch it
        Ok(())
    }

    pub async fn create_note(
        &mut self,
        state: &AppState,
        folder_id: Option<i64>,
    ) -> Result<i64, String> {
        let space_id = *state.space_id.read();
        let id = backend::current()
            .create_note("", "", folder_id, space_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(id)
    }

    pub async fn trash_note(&mut self, state: &AppState, note_id: i64) -> Result<(), String> {
        backend::current()
            .trash_note(note_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_note(&mut self, state: &AppState, note_id: i64) -> Result<(), String> {
        backend::current()
            .restore_note(note_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn move_note(
        &mut self,
        state: &AppState,
        note_id: i64,
        folder_id: Option<i64>,
    ) -> Result<(), String> {
        backend::current()
            .move_note(note_id, folder_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    // ---- Page mutations ----

    pub async fn trash_page(&mut self, state: &AppState, page_id: i64) -> Result<(), String> {
        backend::current()
            .trash_page(page_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_page(&mut self, state: &AppState, page_id: i64) -> Result<(), String> {
        backend::current()
            .restore_page(page_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn retry_page(&mut self, state: &AppState, page_id: i64) -> Result<(), String> {
        backend::current()
            .update_page_status(page_id, "queued")
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn add_url(&mut self, state: &AppState, raw_url: &str) -> Result<String, String> {
        let space_id = *state.space_id.read();
        let outcome = backend::current()
            .archive_url(raw_url, Some(space_id), None, None)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(format!("[{}] {}", outcome.category, raw_url))
    }

    // ---- Folder mutations ----

    pub async fn create_folder(
        &mut self,
        state: &AppState,
        name: &str,
        parent_id: Option<i64>,
    ) -> Result<i64, String> {
        let space_id = *state.space_id.read();
        let id = backend::current()
            .create_folder(name, parent_id, space_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(id)
    }

    pub async fn rename_folder(
        &mut self,
        state: &AppState,
        folder_id: i64,
        name: &str,
    ) -> Result<(), String> {
        backend::current()
            .rename_folder(folder_id, name)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_folder(&mut self, state: &AppState, folder_id: i64) -> Result<(), String> {
        backend::current()
            .delete_folder(folder_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    // ---- Space mutations ----

    pub async fn create_space(&mut self, state: &AppState, name: &str) -> Result<i64, String> {
        let id = backend::current()
            .create_space(name)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(id)
    }

    pub async fn rename_space(
        &mut self,
        state: &AppState,
        space_id: i64,
        name: &str,
    ) -> Result<(), String> {
        backend::current()
            .rename_space(space_id, name)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn trash_space(&mut self, state: &AppState, space_id: i64) -> Result<(), String> {
        backend::current()
            .trash_space(space_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_space(&mut self, state: &AppState, space_id: i64) -> Result<(), String> {
        backend::current()
            .restore_space(space_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_space_permanent(
        &mut self,
        state: &AppState,
        space_id: i64,
    ) -> Result<(), String> {
        backend::current()
            .delete_space_permanent(space_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    // ---- Trash mutations ----

    pub async fn delete_page_permanent(
        &mut self,
        state: &AppState,
        page_id: i64,
    ) -> Result<(), String> {
        backend::current()
            .delete_page_permanent(page_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_note_permanent(
        &mut self,
        state: &AppState,
        note_id: i64,
    ) -> Result<(), String> {
        backend::current()
            .delete_note_permanent(note_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    // ---- File mutations ----

    pub async fn upload_file(
        &mut self,
        state: &AppState,
        name: &str,
        mime_type: Option<&str>,
        data: &[u8],
    ) -> Result<(i64, lore_core::db::InsertFileOutcome), String> {
        let space_id = *state.space_id.read();
        let result = backend::current()
            .insert_file(name, mime_type, data, space_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(result)
    }

    pub async fn trash_file(&mut self, state: &AppState, id: i64) -> Result<(), String> {
        backend::current()
            .trash_file(id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_file(&mut self, state: &AppState, id: i64) -> Result<(), String> {
        backend::current()
            .restore_file(id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_file_permanent(&mut self, state: &AppState, id: i64) -> Result<(), String> {
        backend::current()
            .delete_file_permanent(id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(())
    }

    /// Returns a base64 data URI for inline preview (images and PDFs).
    pub async fn get_file_data_uri(&self, id: i64) -> Option<String> {
        let (mime, bytes) = backend::current().get_file_data(id).await.ok()?;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let mime_str = mime.as_deref().unwrap_or("application/octet-stream");
        Some(format!("data:{};base64,{}", mime_str, b64))
    }

    // ---- URL tracking for current note ----

    pub async fn set_current_note_urls(&mut self, urls: Vec<String>) {
        self.current_note_urls.set(urls);
        let urls = self.current_note_urls.read().clone();
        if urls.is_empty() {
            self.url_statuses.set(HashMap::new());
            return;
        }
        if let Ok(statuses) = backend::current().check_urls_status(&urls).await {
            self.url_statuses.set(statuses);
        }
    }

    pub fn clear_current_note_urls(&mut self) {
        self.current_note_urls.set(Vec::new());
        self.url_statuses.set(HashMap::new());
    }

    // ---- Attachments (images + file blocks) ----

    pub async fn upload_image(
        &mut self,
        note_id: i64,
        name: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<(i64, lore_core::db::InsertAttachmentOutcome), String> {
        backend::current()
            .insert_attachment(note_id, name, mime_type, data)
            .await
            .map_err(|e| e.to_string())
    }

    /// Upload a generic file as a note attachment (file-block, not inline image).
    pub async fn upload_attachment(
        &mut self,
        note_id: i64,
        name: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<(i64, lore_core::db::InsertAttachmentOutcome), String> {
        backend::current()
            .insert_attachment(note_id, name, mime_type, data)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_attachment_data_uri(&self, attachment_id: i64) -> Option<String> {
        let (mime, bytes) = backend::current()
            .get_attachment_data(attachment_id)
            .await
            .ok()?;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Some(format!("data:{};base64,{}", mime, b64))
    }

    pub async fn cleanup_note_attachments(&self, note_id: i64, markdown: &str) {
        // Extract attachment IDs referenced in markdown:
        //   ![...](https://attachment.lore.invalid/123)   (images)
        //   [...](https://attachment.lore.invalid/123)    (file blocks)
        let mut used_ids = Vec::new();
        for part in markdown.split("https://attachment.lore.invalid/") {
            // The number ends at any non-digit char (typically ')')
            let end = part
                .char_indices()
                .find(|(_, c)| !c.is_ascii_digit())
                .map(|(i, _)| i)
                .unwrap_or(part.len());
            if end > 0
                && let Ok(id) = part[..end].parse::<i64>()
            {
                used_ids.push(id);
            }
        }
        backend::current()
            .cleanup_orphaned_attachments(note_id, &used_ids)
            .await
            .ok();
    }

    pub async fn list_active_attachments(&self, note_id: i64) -> Vec<lore_core::db::AttachmentRow> {
        backend::current()
            .list_attachments(note_id)
            .await
            .unwrap_or_default()
    }

    pub async fn list_removed_attachments(
        &self,
        note_id: i64,
    ) -> Vec<lore_core::db::AttachmentRow> {
        backend::current()
            .list_removed_attachments(note_id)
            .await
            .unwrap_or_default()
    }

    /// Restore a soft-deleted attachment. Returns the attachment row so caller
    /// can re-insert the markdown reference at the right place in the note.
    pub async fn restore_attachment(
        &mut self,
        state: &AppState,
        attachment_id: i64,
    ) -> Result<lore_core::db::AttachmentRow, String> {
        let b = backend::current();
        b.restore_attachment(attachment_id)
            .await
            .map_err(|e| e.to_string())?;
        let row = b
            .get_attachment(attachment_id)
            .await
            .map_err(|e| e.to_string())?;
        self.refresh(state).await;
        Ok(row)
    }

    // ---- Auto-archive URLs from note content ----

    pub async fn auto_archive_urls(&self, text: &str, space_id: i64) {
        backend::current()
            .auto_archive_from_text(text, space_id)
            .await
            .ok();
    }
}
