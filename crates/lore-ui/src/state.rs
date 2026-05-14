use dioxus::prelude::*;

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
#[allow(clippy::enum_variant_names)]
pub enum UndoAction {
    RestorePage(i64),
    RestoreNote(i64),
    RestoreFile(i64),
}

/// What's being renamed inline
#[derive(Clone, Debug, PartialEq)]
pub enum Renaming {
    Space(i64, String),  // (space_id, current_name)
    Folder(i64, String), // (folder_id, current_name)
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
    /// Initial state with `space_id = 1` (the always-seeded default space).
    /// `BootedApp` spawns a task to fetch the actual most-recently-used
    /// space from the backend and updates the signal — the value is rarely
    /// visible before the first refresh tick lands.
    pub fn new() -> Self {
        Self {
            section: Signal::new(Section::AllNotes),
            selected: Signal::new(Selected::None),
            search_query: Signal::new(String::new()),
            toast: Signal::new(None),
            refresh_tick: Signal::new(0),
            space_id: Signal::new(1),
            space_dropdown_open: Signal::new(false),
            renaming: Signal::new(None),
        }
    }

    pub fn select_page(&mut self, id: i64) {
        self.selected.set(Selected::Page(id));
    }

    pub fn select_note(&mut self, id: i64) {
        self.selected.set(Selected::Note(id));
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
