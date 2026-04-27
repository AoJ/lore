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
}

/// Global application state, provided via Dioxus context
#[derive(Clone, Copy)]
pub struct AppState {
    pub section: Signal<Section>,
    pub selected: Signal<Selected>,
    pub search_query: Signal<String>,
    pub toast: Signal<Option<ToastData>>,
    /// Bumped to force list refresh after mutations (add URL, delete, restore)
    pub refresh_tick: Signal<u64>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            section: Signal::new(Section::AllPages),
            selected: Signal::new(Selected::None),
            search_query: Signal::new(String::new()),
            toast: Signal::new(None),
            refresh_tick: Signal::new(0),
        }
    }

    pub fn navigate(&mut self, section: Section) {
        self.section.set(section);
        self.selected.set(Selected::None);
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
        drop(current);
        self.refresh_tick.set(current.wrapping_add(1));
    }
}
