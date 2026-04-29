use dioxus::prelude::*;
use crate::data;

/// Which sidebar section is active
#[derive(Clone, Debug, PartialEq)]
pub enum Section {
    AllPages,
    AllNotes,
    AllFiles,
    Search,
    Folder(i64),
    Trash,
    Timeline,
    Settings,
}

/// Which item is selected in the list panel
#[derive(Clone, Debug, PartialEq)]
pub enum Selected {
    None,
    Page(i64),
    Note(i64),
    File(i64),
    SettingsRules,
    SettingsSpaces,
}

/// Toast notification data
#[derive(Clone, Debug)]
pub struct ToastData {
    pub message: String,
    pub undo: Option<UndoAction>,
}

/// What to undo when the user clicks "Undo" in a toast
#[derive(Clone, Debug)]
pub enum UndoAction {
    RestorePage(i64),
    RestoreNote(i64),
    RestoreFile(i64),
}

/// What's being renamed inline
#[derive(Clone, Debug, PartialEq)]
pub enum Renaming {
    Space(i64, String),    // (space_id, current_name)
    Folder(i64, String),   // (folder_id, current_name)
}

/// Global application state, provided via Dioxus context
#[derive(Clone, Copy)]
pub struct AppState {
    pub section: Signal<Section>,
    pub selected: Signal<Selected>,
    pub search_query: Signal<String>,
    pub toast: Signal<Option<ToastData>>,
    pub refresh_tick: Signal<u64>,
    /// Active space ID
    pub space_id: Signal<i64>,
    /// Whether space dropdown is open
    pub space_dropdown_open: Signal<bool>,
    /// Item being renamed inline (space or folder)
    pub renaming: Signal<Option<Renaming>>,
}

impl AppState {
    pub fn new() -> Self {
        // Load active space from DB
        let active_space_id = data::open_db()
            .ok()
            .and_then(|conn| lore_core::db::get_active_space(&conn).ok())
            .map(|s| s.id)
            .unwrap_or(1);

        Self {
            section: Signal::new(Section::AllNotes),
            selected: Signal::new(Selected::None),
            search_query: Signal::new(String::new()),
            toast: Signal::new(None),
            refresh_tick: Signal::new(0),
            space_id: Signal::new(active_space_id),
            space_dropdown_open: Signal::new(false),
            renaming: Signal::new(None),
        }
    }

    pub fn navigate(&mut self, section: Section) {
        self.section.set(section);
        self.selected.set(Selected::None);
        // Note: DataStore.poll() detects section change and refreshes automatically
        // For instant update, caller can also call store.refresh() explicitly
    }

    pub fn select_page(&mut self, id: i64) {
        self.selected.set(Selected::Page(id));
    }

    pub fn select_note(&mut self, id: i64) {
        self.selected.set(Selected::Note(id));
    }

    pub fn switch_space(&mut self, space_id: i64) {
        self.space_id.set(space_id);
        self.section.set(Section::AllPages);
        self.selected.set(Selected::None);
        self.space_dropdown_open.set(false);
        // Touch space in DB
        if let Ok(conn) = data::open_db() {
            lore_core::db::touch_space(&conn, space_id).ok();
        }
        self.bump_refresh();
    }

    pub fn show_toast(&mut self, message: String, undo: Option<UndoAction>) {
        self.toast.set(Some(ToastData { message, undo }));
    }

    pub fn dismiss_toast(&mut self) {
        self.toast.set(None);
    }

    pub fn bump_refresh(&mut self) {
        let current = *self.refresh_tick.read();
        self.refresh_tick.set(current.wrapping_add(1));
    }
}
