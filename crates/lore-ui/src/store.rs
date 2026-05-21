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
use lore_core::error::BackendError;
use std::collections::HashMap;

/// (note_id, title, body, base_title, base_body) queued while offline.
type PendingNoteSave = (i64, String, String, String, String);

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

    // ---- Open-note refresh tracking ----
    /// `Some(id)` while a note is open in the editor. Used by `poll()` to
    /// detect external edits and push a `smartReplace` into the editor.
    pub open_note_id: Signal<Option<i64>>,
    /// Server-side `updated_at` we last loaded into the editor. Compared
    /// against fresh `get_note()` on every poll tick; mismatch triggers a
    /// `smartReplace` and we re-record the new value here.
    pub open_note_updated_at: Signal<Option<String>>,
    /// Number of in-flight `save_note` calls. The poll-tick external-edit
    /// detector skips when this is non-zero — otherwise a poll firing in
    /// the window between a save's server `update_note` completing and
    /// its follow-up `get_note` could see the *new* `updated_at`, treat
    /// our own write as an external edit, and push a stale `smartReplace`
    /// that wipes out keystrokes the user typed during the save.
    pub saves_in_flight: Signal<u32>,
    /// False when the backend is unreachable (network error). Data signals
    /// retain their last-known values until the connection recovers.
    pub backend_online: Signal<bool>,
    /// Keystroke content queued while offline: (note_id, title, body, base_title, base_body).
    /// base_* is the last successfully written server content — merge ancestor on reconnect.
    /// Only the latest content per note is kept; base stays fixed for the whole offline stint.
    pub pending_note_save: Signal<Option<PendingNoteSave>>,

    // ---- Internal ----
    last_poll_rev: Signal<i64>,
    /// Last successfully synced note content: (note_id, title, body).
    /// Updated after every online save_note; used as merge base when first queueing offline.
    last_online_note: Signal<Option<(i64, String, String)>>,
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
            open_note_id: Signal::new(None),
            open_note_updated_at: Signal::new(None),
            saves_in_flight: Signal::new(0),
            backend_online: Signal::new(true),
            pending_note_save: Signal::new(None),
            last_poll_rev: Signal::new(0),
            last_online_note: Signal::new(None),
        }
    }

    /// Called from polling loop — checks DB revision and schema version.
    pub async fn poll(&mut self, state: &AppState) {
        // Schema version check: only flag as outdated when the on-disk DB
        // is *newer* than this build knows about. The previous version
        // tripped on any inequality and on transient `Err`s
        // (`unwrap_or(0)` → `0 != 7` → false positive that stuck forever
        // because the flag never clears). Older schemas just auto-migrate
        // on the next bootstrap, so they're not a banner-worthy event.
        if let Ok(on_disk) = backend::current().db_schema_version().await {
            let known = lore_core::EXPECTED_DB_SCHEMA_VERSION;
            if on_disk > known && !*self.schema_outdated.read() {
                self.schema_outdated.set(true);
            }
        }

        match backend::current().get_revision().await {
            Ok(new_rev) => {
                let was_offline = !*self.backend_online.read();
                if was_offline {
                    self.backend_online.set(true);
                    // Flush queued keystrokes. Fetch the server's current version
                    // first — if it changed while we were offline, 3-way merge
                    // rather than blindly overwriting.
                    let pending = self.pending_note_save.read().clone();
                    if let Some((id, title, body, base_title, base_body)) = pending {
                        let (merged_title, merged_body) =
                            if let Ok(server) = backend::current().get_note(id).await {
                                if server.title == base_title && server.body == base_body {
                                    // Server unchanged — our edits apply cleanly.
                                    (title, body)
                                } else {
                                    // Concurrent edit: 3-way merge on body; prefer
                                    // ours for title (last writer wins on single-line).
                                    use lore_core::merge::three_way_merge;
                                    let bm = three_way_merge(&base_body, &body, &server.body);
                                    let merged_title = if title != base_title {
                                        title
                                    } else {
                                        server.title
                                    };
                                    (merged_title, bm.text)
                                }
                            } else {
                                // Can't confirm server state — use ours.
                                (title, body)
                            };
                        if self
                            .save_note(id, &merged_title, &merged_body)
                            .await
                            .is_ok()
                        {
                            self.pending_note_save.set(None);
                        }
                    }
                }
                if was_offline || new_rev != *self.last_poll_rev.read() {
                    self.last_poll_rev.set(new_rev);
                    self.revision.set(new_rev);
                    self.refresh(state).await;
                }
            }
            Err(_) => {
                self.backend_online.set(false);
                // Keep last_poll_rev and all data signals unchanged so the
                // UI retains its current content while the backend is down.
            }
        }

        // Open-note external-edit refresh. If a note is open and the
        // server's `updated_at` advanced past what we last loaded, push
        // the new content into the editor via `smartReplace` — PM's
        // Mapping carries the cursor over the diff (see `js/index.js`).
        // `save_note` updates `open_note_updated_at` after every write,
        // so our own saves don't trigger this path. Skip while any save
        // is in flight: the server's new `updated_at` would otherwise
        // look like an external edit until our own `get_note` echo lands.
        if let Some(id) = *self.open_note_id.read()
            && *self.saves_in_flight.read() == 0
            && let Ok(latest) = backend::current().get_note(id).await
        {
            let known = self.open_note_updated_at.read().clone();
            if known.as_deref() != Some(latest.updated_at.as_str()) {
                let md = if latest.title.is_empty() && latest.body.is_empty() {
                    String::new()
                } else if latest.body.is_empty() {
                    latest.title.clone()
                } else {
                    format!("{}\n{}", latest.title, latest.body)
                };
                let md_json = serde_json::to_string(&md).unwrap_or_else(|_| "\"\"".to_string());
                let js = format!(
                    "window.loreEditor && window.loreEditor.smartReplace({});",
                    md_json
                );
                dioxus::document::eval(&js);
                self.open_note_updated_at.set(Some(latest.updated_at));
            }
        }
    }

    /// Refresh all data for current view. Called after mutations and on poll.
    /// No-op while the backend is offline — cached data is preserved as-is.
    pub async fn refresh(&mut self, state: &AppState) {
        if !*self.backend_online.read() {
            return;
        }
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

    pub async fn save_note(
        &mut self,
        note_id: i64,
        title: &str,
        body: &str,
    ) -> Result<(), BackendError> {
        if !*self.backend_online.read() {
            // Determine the merge base: the last successfully written content for
            // this note. On subsequent offline keystrokes the base stays fixed so
            // the merge ancestor doesn't drift as the user types.
            let existing = self.pending_note_save.read().clone();
            let (base_title, base_body) = match existing {
                Some((eid, _, _, bt, bb)) if eid == note_id => (bt, bb),
                _ => {
                    if let Some((lid, lt, lb)) = self.last_online_note.read().clone()
                        && lid == note_id
                    {
                        (lt, lb)
                    } else {
                        // No cached base — use current content (no-conflict merge).
                        (title.to_string(), body.to_string())
                    }
                }
            };
            self.pending_note_save.set(Some((
                note_id,
                title.to_string(),
                body.to_string(),
                base_title,
                base_body,
            )));
            return Ok(());
        }
        let b = backend::current();
        // Increment-on-entry / decrement-on-exit so `poll()` skips the
        // external-edit detector for the full duration of the round-trip
        // (including the follow-up `get_note`). Without this, a poll
        // firing between `update_note` and `get_note` would see the new
        // `updated_at` while `open_note_updated_at` still holds the old
        // value and would push a stale `smartReplace`.
        let mut counter = self.saves_in_flight;
        let entry_count = *counter.read();
        counter.set(entry_count + 1);

        let outcome = async {
            b.update_note(note_id, title, body).await?;
            // If this is the open note, refresh our cached `updated_at`
            // so the next poll's comparison is against the value our
            // own write produced.
            if *self.open_note_id.read() == Some(note_id)
                && let Ok(n) = b.get_note(note_id).await
            {
                self.open_note_updated_at.set(Some(n.updated_at));
            }
            // Cache the written content so we have a merge base if we go offline.
            self.last_online_note
                .set(Some((note_id, title.to_string(), body.to_string())));
            // Don't refresh list on every keystroke — polling will catch it
            Ok::<(), BackendError>(())
        }
        .await;

        let exit_count = *counter.read();
        counter.set(exit_count.saturating_sub(1));
        outcome
    }

    pub async fn create_note(
        &mut self,
        state: &AppState,
        folder_id: Option<i64>,
    ) -> Result<i64, BackendError> {
        let space_id = *state.space_id.read();
        let id = backend::current()
            .create_note("", "", folder_id, space_id)
            .await?;
        self.refresh(state).await;
        Ok(id)
    }

    pub async fn trash_note(&mut self, state: &AppState, note_id: i64) -> Result<(), BackendError> {
        backend::current().trash_note(note_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_note(
        &mut self,
        state: &AppState,
        note_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().restore_note(note_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn move_note(
        &mut self,
        state: &AppState,
        note_id: i64,
        folder_id: Option<i64>,
    ) -> Result<(), BackendError> {
        backend::current().move_note(note_id, folder_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    // ---- Page mutations ----

    pub async fn trash_page(&mut self, state: &AppState, page_id: i64) -> Result<(), BackendError> {
        backend::current().trash_page(page_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_page(
        &mut self,
        state: &AppState,
        page_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().restore_page(page_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn retry_page(&mut self, state: &AppState, page_id: i64) -> Result<(), BackendError> {
        backend::current()
            .update_page_status(page_id, "queued")
            .await?;
        self.refresh(state).await;
        Ok(())
    }

    /// Queue a fresh fetch for an already-archived page. Worker picks it up
    /// on next run; new snapshot lands as a new version (existing ones kept).
    pub async fn reachive_page(
        &mut self,
        state: &AppState,
        page_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().request_reachive(page_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    /// Delete one historical snapshot version. Backend refuses if it's the
    /// only one — UI shouldn't even show the button in that case.
    pub async fn delete_page_version(
        &mut self,
        state: &mut AppState,
        snapshot_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().delete_page_version(snapshot_id).await?;
        // Revision bump from FTS+snapshot delete will trigger polling refresh
        // automatically, but we want the open page detail to update now.
        state.bump_refresh();
        Ok(())
    }

    pub async fn add_url(
        &mut self,
        state: &AppState,
        raw_url: &str,
    ) -> Result<String, BackendError> {
        let space_id = *state.space_id.read();
        let outcome = backend::current()
            .archive_url(raw_url, Some(space_id), None, None)
            .await?;
        self.refresh(state).await;
        Ok(format!("[{}] {}", outcome.category, raw_url))
    }

    // ---- Folder mutations ----

    pub async fn create_folder(
        &mut self,
        state: &AppState,
        name: &str,
        parent_id: Option<i64>,
    ) -> Result<i64, BackendError> {
        let space_id = *state.space_id.read();
        let id = backend::current()
            .create_folder(name, parent_id, space_id)
            .await?;
        self.refresh(state).await;
        Ok(id)
    }

    pub async fn rename_folder(
        &mut self,
        state: &AppState,
        folder_id: i64,
        name: &str,
    ) -> Result<(), BackendError> {
        backend::current().rename_folder(folder_id, name).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_folder(
        &mut self,
        state: &AppState,
        folder_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().delete_folder(folder_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    // ---- Space mutations ----

    pub async fn create_space(
        &mut self,
        state: &AppState,
        name: &str,
    ) -> Result<i64, BackendError> {
        let id = backend::current().create_space(name).await?;
        self.refresh(state).await;
        Ok(id)
    }

    pub async fn rename_space(
        &mut self,
        state: &AppState,
        space_id: i64,
        name: &str,
    ) -> Result<(), BackendError> {
        backend::current().rename_space(space_id, name).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn trash_space(
        &mut self,
        state: &AppState,
        space_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().trash_space(space_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_space(
        &mut self,
        state: &AppState,
        space_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().restore_space(space_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_space_permanent(
        &mut self,
        state: &AppState,
        space_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().delete_space_permanent(space_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    // ---- Trash mutations ----

    pub async fn delete_page_permanent(
        &mut self,
        state: &AppState,
        page_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().delete_page_permanent(page_id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_note_permanent(
        &mut self,
        state: &AppState,
        note_id: i64,
    ) -> Result<(), BackendError> {
        backend::current().delete_note_permanent(note_id).await?;
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
    ) -> Result<(i64, lore_core::db::InsertFileOutcome), BackendError> {
        let space_id = *state.space_id.read();
        let result = backend::current()
            .insert_file(name, mime_type, data, space_id)
            .await?;
        self.refresh(state).await;
        Ok(result)
    }

    pub async fn trash_file(&mut self, state: &AppState, id: i64) -> Result<(), BackendError> {
        backend::current().trash_file(id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn restore_file(&mut self, state: &AppState, id: i64) -> Result<(), BackendError> {
        backend::current().restore_file(id).await?;
        self.refresh(state).await;
        Ok(())
    }

    pub async fn delete_file_permanent(
        &mut self,
        state: &AppState,
        id: i64,
    ) -> Result<(), BackendError> {
        backend::current().delete_file_permanent(id).await?;
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
    ) -> Result<(i64, lore_core::db::InsertAttachmentOutcome), BackendError> {
        backend::current()
            .insert_attachment(note_id, name, mime_type, data)
            .await
    }

    /// Upload a generic file as a note attachment (file-block, not inline image).
    pub async fn upload_attachment(
        &mut self,
        note_id: i64,
        name: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<(i64, lore_core::db::InsertAttachmentOutcome), BackendError> {
        backend::current()
            .insert_attachment(note_id, name, mime_type, data)
            .await
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

    pub async fn cleanup_note_attachments(&mut self, note_id: i64, markdown: &str) {
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
        if backend::current()
            .cleanup_orphaned_attachments(note_id, &used_ids)
            .await
            .is_ok()
        {
            // Bump revision signal so RemovedAttachments re-fetches immediately
            // (cleanup may have soft-deleted attachments removed from markdown).
            if let Ok(new_rev) = backend::current().get_revision().await
                && new_rev != *self.revision.read()
            {
                self.revision.set(new_rev);
            }
        }
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
    ) -> Result<lore_core::db::AttachmentRow, BackendError> {
        let b = backend::current();
        b.restore_attachment(attachment_id).await?;
        let row = b.get_attachment(attachment_id).await?;
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
