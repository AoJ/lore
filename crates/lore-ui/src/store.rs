//! Centralized data store — single source of truth for all UI data.
//! Components read from signals here, never call DB directly.
//! Mutations go through store methods which update DB + signals atomically.

use dioxus::prelude::*;
use std::collections::HashMap;
use crate::data::{self, PageRow, PageDetailData, TrashItem, TrashKind, RuleRow};
use crate::state::{AppState, Section};

/// Central data store, provided as Dioxus context alongside AppState.
#[derive(Clone, Copy)]
pub struct DataStore {
    // ---- Cached data (read by components) ----
    pub pages: Signal<Vec<PageRow>>,
    pub notes: Signal<Vec<lore_core::db::NoteRow>>,
    pub folders: Signal<Vec<lore_core::db::FolderRow>>,
    pub spaces: Signal<Vec<lore_core::db::SpaceRow>>,
    pub trash_items: Signal<Vec<TrashItem>>,
    pub trash_count: Signal<i64>,
    pub note_counts: Signal<HashMap<i64, i64>>,
    pub revision: Signal<i64>,

    // ---- Internal ----
    last_poll_rev: Signal<i64>,
}

impl DataStore {
    pub fn new(space_id: i64) -> Self {
        let rev = data::get_revision();
        Self {
            pages: Signal::new(Vec::new()),
            notes: Signal::new(Vec::new()),
            folders: Signal::new(Vec::new()),
            spaces: Signal::new(Vec::new()),
            trash_items: Signal::new(Vec::new()),
            trash_count: Signal::new(0),
            note_counts: Signal::new(HashMap::new()),
            revision: Signal::new(rev),
            last_poll_rev: Signal::new(rev),
        }
    }

    /// Check if DB revision changed and refresh if needed. Called from polling loop.
    pub fn poll(&mut self, state: &AppState) {
        let new_rev = data::get_revision();
        if new_rev != *self.last_poll_rev.read() {
            self.last_poll_rev.set(new_rev);
            self.revision.set(new_rev);
            self.refresh(state);
        }
    }

    /// Refresh all data for current view. Called after mutations and on poll.
    pub fn refresh(&mut self, state: &AppState) {
        let space_id = *state.space_id.read();
        let section = state.section.read().clone();

        let conn = match data::open_db() {
            Ok(c) => c,
            Err(_) => return,
        };

        // Always refresh: spaces, folders, counts, trash
        self.spaces.set(lore_core::db::list_spaces(&conn).unwrap_or_default());
        self.folders.set(lore_core::db::list_folders(&conn, space_id).unwrap_or_default());
        self.note_counts.set(lore_core::db::folder_note_counts(&conn, space_id).unwrap_or_default());
        self.trash_count.set(lore_core::db::trash_count(&conn).unwrap_or(0));

        // Refresh list for current section
        match section {
            Section::AllPages => {
                self.pages.set(data::list_pages(space_id, 200).unwrap_or_default());
            }
            Section::AllNotes => {
                self.notes.set(lore_core::db::list_notes(&conn, None, space_id).unwrap_or_default());
            }
            Section::Folder(folder_id) => {
                self.notes.set(lore_core::db::list_notes(&conn, Some(folder_id), space_id).unwrap_or_default());
            }
            Section::Trash => {
                self.trash_items.set(data::list_trash(space_id).unwrap_or_default());
            }
            _ => {}
        }

        self.revision.set(data::get_revision());
    }

    // ---- Note mutations ----

    pub fn save_note(&mut self, note_id: i64, title: &str, body: &str) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::update_note(&conn, note_id, title, body).map_err(|e| e.to_string())?;
        // Don't refresh list on every keystroke — polling will catch it
        Ok(())
    }

    pub fn create_note(&mut self, state: &AppState, folder_id: Option<i64>) -> Result<i64, String> {
        let space_id = *state.space_id.read();
        let conn = data::open_db().map_err(|e| e.to_string())?;
        let id = lore_core::db::insert_note(&conn, "", "", folder_id, space_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(id)
    }

    pub fn trash_note(&mut self, state: &AppState, note_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::trash_note(&conn, note_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn restore_note(&mut self, state: &AppState, note_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::restore_note_safe(&conn, note_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn move_note(&mut self, state: &AppState, note_id: i64, folder_id: Option<i64>) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::move_note_to_folder(&conn, note_id, folder_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    // ---- Page mutations ----

    pub fn trash_page(&mut self, state: &AppState, page_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::trash_page(&conn, page_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn restore_page(&mut self, state: &AppState, page_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::restore_page(&conn, page_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn retry_page(&mut self, state: &AppState, page_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::update_status(&conn, page_id, "queued").map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn add_url(&mut self, state: &AppState, raw_url: &str) -> Result<String, String> {
        let space_id = *state.space_id.read();
        let result = data::add_url(raw_url, space_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(result)
    }

    // ---- Folder mutations ----

    pub fn create_folder(&mut self, state: &AppState, name: &str, parent_id: Option<i64>) -> Result<i64, String> {
        let space_id = *state.space_id.read();
        let conn = data::open_db().map_err(|e| e.to_string())?;
        let id = lore_core::db::insert_folder(&conn, name, parent_id, space_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(id)
    }

    pub fn rename_folder(&mut self, state: &AppState, folder_id: i64, name: &str) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::rename_folder(&conn, folder_id, name).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn delete_folder(&mut self, state: &AppState, folder_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::delete_folder(&conn, folder_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    // ---- Space mutations ----

    pub fn create_space(&mut self, state: &AppState, name: &str) -> Result<i64, String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        let id = lore_core::db::insert_space(&conn, name).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(id)
    }

    pub fn rename_space(&mut self, state: &AppState, space_id: i64, name: &str) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::rename_space(&conn, space_id, name).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn trash_space(&mut self, state: &AppState, space_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::trash_space(&conn, space_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn restore_space(&mut self, state: &AppState, space_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::restore_space(&conn, space_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn delete_space_permanent(&mut self, state: &AppState, space_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::delete_space_permanent(&conn, space_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    // ---- Trash mutations ----

    pub fn delete_page_permanent(&mut self, state: &AppState, page_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::delete_page(&conn, page_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    pub fn delete_note_permanent(&mut self, state: &AppState, note_id: i64) -> Result<(), String> {
        let conn = data::open_db().map_err(|e| e.to_string())?;
        lore_core::db::delete_note_permanent(&conn, note_id).map_err(|e| e.to_string())?;
        self.refresh(state);
        Ok(())
    }

    // ---- Auto-archive URLs from note content ----

    pub fn auto_archive_urls(&self, text: &str, space_id: i64) {
        data::auto_archive_urls(text, space_id);
    }
}
